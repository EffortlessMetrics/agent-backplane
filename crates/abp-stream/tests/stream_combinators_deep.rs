// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec)]
//! Comprehensive tests for stream processing patterns and combinators.

use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventRecorder, EventStats, EventStream,
    EventTransform, MergedStream, StreamAggregator, StreamBuffer, StreamMetrics,
    StreamPipelineBuilder, StreamTee, TimeoutStream,
};
use chrono::Utc;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

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

fn tool_call(name: &str, id: Option<&str>) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: id.map(|s| s.to_string()),
        parent_tool_use_id: None,
        input: serde_json::json!({"arg": "value"}),
    })
}

fn tool_result(name: &str, id: Option<&str>, output: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: id.map(|s| s.to_string()),
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

fn command_executed(cmd: &str, code: i32) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(code),
        output_preview: None,
    })
}

/// Builds a stream from a Vec of events using an mpsc channel.
fn vec_stream(events: Vec<AgentEvent>) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
    let (tx, rx) = mpsc::channel(events.len().max(1));
    tokio::spawn(async move {
        for ev in events {
            let _ = tx.send(ev).await;
        }
    });
    Box::pin(EventStream::new(rx))
}

// ===========================================================================
// 1. Text Accumulation
// ===========================================================================

#[tokio::test]
async fn text_accum_single_delta() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    assert_eq!(agg.text(), "hello");
}

#[tokio::test]
async fn text_accum_multiple_deltas() {
    let mut agg = StreamAggregator::new();
    for chunk in &["Hello", ", ", "world", "!"] {
        agg.push(&delta(chunk));
    }
    assert_eq!(agg.text(), "Hello, world!");
}

#[tokio::test]
async fn text_accum_empty_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta(""));
    agg.push(&delta("text"));
    agg.push(&delta(""));
    assert_eq!(agg.text(), "text");
}

#[tokio::test]
async fn text_accum_message_event() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("full message"));
    assert_eq!(agg.text(), "full message");
}

#[tokio::test]
async fn text_accum_mixed_delta_and_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("part1 "));
    agg.push(&message("part2"));
    assert_eq!(agg.text(), "part1 part2");
}

#[tokio::test]
async fn text_accum_unicode_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("こんに"));
    agg.push(&delta("ちは"));
    agg.push(&delta(" 🌍"));
    assert_eq!(agg.text(), "こんにちは 🌍");
}

#[tokio::test]
async fn text_accum_large_stream() {
    let mut agg = StreamAggregator::new();
    for i in 0..1000 {
        agg.push(&delta(&format!("t{i}")));
    }
    assert_eq!(agg.event_count(), 1000);
    assert!(agg.text().starts_with("t0t1t2"));
}

// ===========================================================================
// 2. Tool Call Accumulation
// ===========================================================================

#[tokio::test]
async fn tool_call_single() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", Some("tc1")));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert_eq!(agg.tool_calls()[0].id, "tc1");
}

#[tokio::test]
async fn tool_call_with_result() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", Some("tc1")));
    agg.push(&tool_result("read_file", Some("tc1"), "file contents"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].result.is_some());
}

#[tokio::test]
async fn tool_call_multiple_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", Some("tc1")));
    agg.push(&tool_call("write_file", Some("tc2")));
    agg.push(&tool_call("search", Some("tc3")));
    assert_eq!(agg.tool_calls().len(), 3);
    assert_eq!(agg.tool_calls()[2].name, "search");
}

#[tokio::test]
async fn tool_call_without_id_generates_synthetic() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("my_tool", None));
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].id.starts_with("my_tool_"));
}

#[tokio::test]
async fn tool_call_result_without_matching_call() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result("orphan", Some("nope"), "data"));
    // Should not crash; no tool call to attach to
    assert_eq!(agg.tool_calls().len(), 0);
    assert_eq!(agg.event_count(), 1);
}

// ===========================================================================
// 3. Event Filtering
// ===========================================================================

#[tokio::test]
async fn filter_by_kind_delta() {
    let filter = EventFilter::by_kind("assistant_delta");
    assert!(filter.matches(&delta("hi")));
    assert!(!filter.matches(&error_event("err")));
    assert!(!filter.matches(&run_started()));
}

#[tokio::test]
async fn filter_errors_only() {
    let filter = EventFilter::errors_only();
    assert!(filter.matches(&error_event("bad")));
    assert!(!filter.matches(&delta("ok")));
    assert!(!filter.matches(&warning_event("warn")));
}

#[tokio::test]
async fn filter_exclude_errors() {
    let filter = EventFilter::exclude_errors();
    assert!(!filter.matches(&error_event("bad")));
    assert!(filter.matches(&delta("ok")));
    assert!(filter.matches(&run_completed()));
}

#[tokio::test]
async fn filter_custom_predicate() {
    let filter = EventFilter::new(|ev| {
        matches!(
            &ev.kind,
            AgentEventKind::AssistantDelta { text } if text.len() > 3
        )
    });
    assert!(filter.matches(&delta("long text")));
    assert!(!filter.matches(&delta("hi")));
}

#[tokio::test]
async fn filter_collect_filtered_stream() {
    let (tx, rx) = mpsc::channel(10);
    let stream = EventStream::new(rx);

    tokio::spawn(async move {
        let _ = tx.send(delta("hello")).await;
        let _ = tx.send(error_event("oops")).await;
        let _ = tx.send(delta("world")).await;
        let _ = tx.send(warning_event("warn")).await;
    });

    let filter = EventFilter::by_kind("assistant_delta");
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn filter_chained_in_pipeline() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();

    assert!(pipeline.process(delta("ok")).is_some());
    assert!(pipeline.process(error_event("nope")).is_none());
    assert!(pipeline.process(run_started()).is_none());
}

// ===========================================================================
// 4. Event Mapping / Transform
// ===========================================================================

#[tokio::test]
async fn transform_identity() {
    let transform = EventTransform::identity();
    let ev = delta("hi");
    let result = transform.apply(ev.clone());
    assert_eq!(result.ts, ev.ts);
}

#[tokio::test]
async fn transform_modify_event() {
    let transform = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let result = transform.apply(delta("hello"));
    if let AgentEventKind::AssistantDelta { text } = &result.kind {
        assert_eq!(text, "HELLO");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[tokio::test]
async fn transform_in_pipeline() {
    let transform = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            text.push_str(" [transformed]");
        }
        ev
    });
    let pipeline = StreamPipelineBuilder::new().transform(transform).build();
    let result = pipeline.process(delta("data")).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &result.kind {
        assert!(text.contains("[transformed]"));
    } else {
        panic!("expected AssistantDelta");
    }
}

#[tokio::test]
async fn transform_chained_multiple() {
    let t1 = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            text.push('!');
        }
        ev
    });
    let pipeline = StreamPipelineBuilder::new()
        .transform(t1)
        .transform(t2)
        .build();
    let result = pipeline.process(delta("hi")).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &result.kind {
        assert_eq!(text, "HI!");
    } else {
        panic!("expected AssistantDelta");
    }
}

// ===========================================================================
// 5. Buffer Strategies
// ===========================================================================

#[tokio::test]
async fn buffer_fixed_capacity() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert!(buf.is_full());
    assert_eq!(buf.len(), 3);
}

#[tokio::test]
async fn buffer_evicts_oldest() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(2);
    if let AgentEventKind::AssistantDelta { text } = &recent[0].kind {
        assert_eq!(text, "b");
    }
}

#[tokio::test]
async fn buffer_recent_fewer_than_available() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    assert_eq!(buf.recent(5).len(), 2);
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

#[tokio::test]
async fn buffer_capacity_one() {
    let mut buf = StreamBuffer::new(1);
    buf.push(delta("a"));
    buf.push(delta("b"));
    assert_eq!(buf.len(), 1);
    if let AgentEventKind::AssistantDelta { text } = &buf.recent(1)[0].kind {
        assert_eq!(text, "b");
    }
}

#[tokio::test]
async fn buffered_stream_batch_by_size() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        for i in 0..6 {
            let _ = tx.send(delta(&format!("d{i}"))).await;
        }
    });
    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    tokio::pin!(buffered);
    let batch1 = buffered.next().await.unwrap();
    assert_eq!(batch1.len(), 3);
    let batch2 = buffered.next().await.unwrap();
    assert_eq!(batch2.len(), 3);
}

#[tokio::test]
async fn buffered_stream_remainder_on_close() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        for i in 0..5 {
            let _ = tx.send(delta(&format!("d{i}"))).await;
        }
    });
    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    tokio::pin!(buffered);
    let mut batches = Vec::new();
    while let Some(batch) = buffered.next().await {
        batches.push(batch);
    }
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 2);
}

#[tokio::test]
async fn buffered_stream_single_element_batches() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let _ = tx.send(delta("a")).await;
        let _ = tx.send(delta("b")).await;
    });
    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 1);
    tokio::pin!(buffered);
    let mut batches = Vec::new();
    while let Some(batch) = buffered.next().await {
        batches.push(batch);
    }
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 1);
}

// ===========================================================================
// 6. Backpressure
// ===========================================================================

#[tokio::test]
async fn backpressure_bounded_channel() {
    // Channel size 2 forces backpressure after 2 items
    let (tx, mut rx) = mpsc::channel(2);
    let tx_handle = tokio::spawn(async move {
        let mut sent = 0;
        for i in 0..10 {
            if tx.send(delta(&format!("msg{i}"))).await.is_ok() {
                sent += 1;
            }
        }
        sent
    });

    // Slow consumer: drain after producer tries to fill
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut received = 0;
    while rx.recv().await.is_some() {
        received += 1;
    }

    let sent = tx_handle.await.unwrap();
    assert_eq!(sent, received);
}

#[tokio::test]
async fn backpressure_tee_with_slow_receiver() {
    let (tx1, mut rx1) = mpsc::channel(100);
    let (tx2, mut rx2) = mpsc::channel(100);
    let tee = StreamTee::new(vec![tx1, tx2]);

    for i in 0..3 {
        let _ = tee.send(&delta(&format!("e{i}"))).await;
    }
    drop(tee);

    // Both receivers should get all 3 events
    let mut count1 = 0;
    while rx1.try_recv().is_ok() {
        count1 += 1;
    }
    let mut count2 = 0;
    while rx2.try_recv().is_ok() {
        count2 += 1;
    }
    assert_eq!(count1, 3);
    assert_eq!(count2, 3);
}

#[tokio::test]
async fn backpressure_pipeline_does_not_drop() {
    let (tx, rx) = mpsc::channel(4);
    let (out_tx, mut out_rx) = mpsc::channel(4);

    let pipeline = StreamPipelineBuilder::new().build();
    let stream = EventStream::new(rx);

    tokio::spawn(async move {
        stream.pipe(&pipeline, out_tx).await;
    });

    for i in 0..4 {
        tx.send(delta(&format!("d{i}"))).await.unwrap();
    }
    drop(tx);

    let mut count = 0;
    while out_rx.recv().await.is_some() {
        count += 1;
    }
    assert_eq!(count, 4);
}

// ===========================================================================
// 7. Timeout Handling
// ===========================================================================

#[tokio::test]
async fn timeout_fires_on_no_events() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(1);
    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_millis(50));
    tokio::pin!(ts);

    let item = ts.next().await.unwrap();
    assert!(item.is_err());
}

#[tokio::test]
async fn timeout_resets_on_event() {
    let (tx, rx) = mpsc::channel(10);
    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_millis(200));
    tokio::pin!(ts);

    // Keep tx alive in a spawned task so stream stays open (pending, not closed)
    tokio::spawn(async move {
        let _ = tx.send(delta("a")).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = tx.send(delta("b")).await;
        // Keep tx alive so the stream doesn't close
        tokio::time::sleep(Duration::from_secs(10)).await;
    });

    let item1 = ts.next().await.unwrap();
    assert!(item1.is_ok());
    let item2 = ts.next().await.unwrap();
    assert!(item2.is_ok());
    // Third should timeout because no more events are sent
    let item3 = ts.next().await.unwrap();
    assert!(item3.is_err());
}

#[tokio::test]
async fn timeout_stream_completes_normally() {
    let (tx, rx) = mpsc::channel(10);
    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_secs(5));
    tokio::pin!(ts);
    tokio::spawn(async move {
        let _ = tx.send(delta("a")).await;
        // Drop tx to close stream
    });

    let item1 = ts.next().await.unwrap();
    assert!(item1.is_ok());
    // Stream closed → None
    let item2 = ts.next().await;
    assert!(item2.is_none());
}

#[tokio::test]
async fn timeout_error_contains_duration() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(1);
    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_millis(25));
    tokio::pin!(ts);

    if let Some(Err(e)) = ts.next().await {
        assert_eq!(e.duration, Duration::from_millis(25));
        let display = format!("{e}");
        assert!(display.contains("timeout"));
    } else {
        panic!("expected timeout error");
    }
}

// ===========================================================================
// 8. Error Propagation
// ===========================================================================

#[tokio::test]
async fn error_mid_stream_aggregated() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("start"));
    agg.push(&error_event("something broke"));
    agg.push(&delta("continue"));

    assert_eq!(agg.text(), "startcontinue");
    assert_eq!(agg.errors().len(), 1);
    let summary = agg.to_summary();
    assert!(summary.has_errors);
}

#[tokio::test]
async fn error_multiple_errors_collected() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_event("e1"));
    agg.push(&error_event("e2"));
    agg.push(&error_event("e3"));
    assert_eq!(agg.errors().len(), 3);
}

#[tokio::test]
async fn error_pipeline_filters_errors() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    assert!(pipeline.process(delta("ok")).is_some());
    assert!(pipeline.process(error_event("nope")).is_none());
}

#[tokio::test]
async fn error_pipeline_passes_only_errors() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();

    assert!(pipeline.process(error_event("yes")).is_some());
    assert!(pipeline.process(delta("nope")).is_none());
}

#[tokio::test]
async fn error_stats_counts_errors() {
    let stats = EventStats::new();
    stats.observe(&delta("ok"));
    stats.observe(&error_event("e1"));
    stats.observe(&error_event("e2"));
    assert_eq!(stats.error_count(), 2);
    assert_eq!(stats.total_events(), 3);
}

// ===========================================================================
// 9. Stream Merging
// ===========================================================================

#[tokio::test]
async fn merge_two_streams() {
    let s1 = vec_stream(vec![delta("a"), delta("b")]);
    let s2 = vec_stream(vec![delta("c"), delta("d")]);

    let mut merged = MergedStream::new(vec![s1, s2]);
    let mut items = Vec::new();
    while let Some(item) = merged.next().await {
        items.push(item);
    }
    assert_eq!(items.len(), 4);
}

#[tokio::test]
async fn merge_empty_streams() {
    let merged = MergedStream::new(vec![]);
    let items: Vec<_> = merged.collect().await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn merge_one_empty_one_full() {
    let s1 = vec_stream(vec![]);
    let s2 = vec_stream(vec![delta("x"), delta("y")]);

    let merged = MergedStream::new(vec![s1, s2]);
    let items: Vec<_> = merged.collect().await;
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn merge_three_streams() {
    let s1 = vec_stream(vec![delta("a")]);
    let s2 = vec_stream(vec![delta("b")]);
    let s3 = vec_stream(vec![delta("c")]);

    let merged = MergedStream::new(vec![s1, s2, s3]);
    let items: Vec<_> = merged.collect().await;
    assert_eq!(items.len(), 3);
}

#[tokio::test]
async fn merge_remaining_count() {
    let s1 = vec_stream(vec![delta("a")]);
    let s2 = vec_stream(vec![delta("b")]);
    let merged = MergedStream::new(vec![s1, s2]);
    assert_eq!(merged.remaining(), 2);
}

#[tokio::test]
async fn multiplexer_collect_sorted() {
    use abp_stream::EventMultiplexer;

    let (tx1, rx1) = mpsc::channel(10);
    let (tx2, rx2) = mpsc::channel(10);

    let ts1 = Utc::now();
    let ts2 = ts1 + chrono::Duration::milliseconds(100);

    tx1.send(AgentEvent {
        ts: ts2,
        kind: AgentEventKind::AssistantDelta {
            text: "second".to_string(),
        },
        ext: None,
    })
    .await
    .unwrap();
    tx2.send(AgentEvent {
        ts: ts1,
        kind: AgentEventKind::AssistantDelta {
            text: "first".to_string(),
        },
        ext: None,
    })
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let sorted = mux.collect_sorted().await;
    assert_eq!(sorted.len(), 2);
    if let AgentEventKind::AssistantDelta { text } = &sorted[0].kind {
        assert_eq!(text, "first");
    }
}

// ===========================================================================
// 10. Stream Splitting (by event kind)
// ===========================================================================

#[tokio::test]
async fn split_by_kind_using_pipeline() {
    let delta_pipe = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    let error_pipe = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();

    let events = vec![delta("a"), error_event("e"), delta("b"), warning_event("w")];

    let deltas: Vec<_> = events
        .iter()
        .cloned()
        .filter_map(|ev| delta_pipe.process(ev))
        .collect();
    let errors: Vec<_> = events
        .into_iter()
        .filter_map(|ev| error_pipe.process(ev))
        .collect();

    assert_eq!(deltas.len(), 2);
    assert_eq!(errors.len(), 1);
}

#[tokio::test]
async fn split_tee_to_different_consumers() {
    let (tx1, mut rx1) = mpsc::channel(10);
    let (tx2, mut rx2) = mpsc::channel(10);
    let tee = StreamTee::new(vec![tx1, tx2]);

    tee.send(&delta("shared")).await.unwrap();
    drop(tee);

    let ev1 = rx1.recv().await.unwrap();
    let ev2 = rx2.recv().await.unwrap();
    if let (
        AgentEventKind::AssistantDelta { text: t1 },
        AgentEventKind::AssistantDelta { text: t2 },
    ) = (&ev1.kind, &ev2.kind)
    {
        assert_eq!(t1, "shared");
        assert_eq!(t2, "shared");
    }
}

#[tokio::test]
async fn split_collect_by_kind() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let _ = tx.send(run_started()).await;
        let _ = tx.send(delta("text")).await;
        let _ = tx.send(tool_call("search", Some("t1"))).await;
        let _ = tx.send(error_event("oops")).await;
        let _ = tx.send(run_completed()).await;
    });

    let stream = EventStream::new(rx);
    let all = stream.collect_all().await;

    let deltas: Vec<_> = all
        .iter()
        .filter(|ev| matches!(ev.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    let tools: Vec<_> = all
        .iter()
        .filter(|ev| matches!(ev.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    let errors: Vec<_> = all
        .iter()
        .filter(|ev| matches!(ev.kind, AgentEventKind::Error { .. }))
        .collect();

    assert_eq!(deltas.len(), 1);
    assert_eq!(tools.len(), 1);
    assert_eq!(errors.len(), 1);
}

// ===========================================================================
// 11. Rate Limiting (simulated via timing assertions)
// ===========================================================================

#[tokio::test]
async fn rate_limit_via_buffered_flush_interval() {
    let (tx, rx) = mpsc::channel(20);
    tokio::spawn(async move {
        for i in 0..4 {
            let _ = tx.send(delta(&format!("d{i}"))).await;
        }
    });

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 100).with_flush_interval(Duration::from_millis(50));
    tokio::pin!(buffered);
    let mut batches = Vec::new();
    while let Some(batch) = buffered.next().await {
        batches.push(batch);
    }

    // Should flush by interval or on close, not wait for 100 items
    assert!(!batches.is_empty());
    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, 4);
}

#[tokio::test]
async fn rate_limit_throttle_emission() {
    let (tx, rx) = mpsc::channel(20);
    tokio::spawn(async move {
        for i in 0..5 {
            let _ = tx.send(delta(&format!("d{i}"))).await;
        }
    });

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 2);
    tokio::pin!(buffered);
    let mut batches = Vec::new();
    while let Some(batch) = buffered.next().await {
        batches.push(batch);
    }

    // 5 events in batches of 2 → 3 batches (2+2+1)
    assert_eq!(batches.len(), 3);
}

// ===========================================================================
// 12. Completion Detection
// ===========================================================================

#[tokio::test]
async fn completion_detected_by_aggregator() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_started());
    assert!(!agg.is_complete());
    agg.push(&delta("text"));
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[tokio::test]
async fn completion_summary_after_full_stream() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("hello"));
    agg.push(&delta(" world"));
    agg.push(&tool_call("t", Some("id1")));
    agg.push(&run_completed());

    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 5);
    assert_eq!(summary.text_length, 11);
    assert_eq!(summary.tool_call_count, 1);
    assert!(!summary.has_errors);
}

#[tokio::test]
async fn completion_event_stream_ends_on_drop() {
    let (tx, rx) = mpsc::channel(10);
    let mut stream = EventStream::new(rx);

    tx.send(delta("a")).await.unwrap();
    drop(tx);

    let item = stream.recv().await;
    assert!(item.is_some());
    let item = stream.recv().await;
    assert!(item.is_none());
}

#[tokio::test]
async fn completion_collect_all_drains() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let _ = tx.send(delta("a")).await;
        let _ = tx.send(delta("b")).await;
    });

    let stream = EventStream::new(rx);
    let all = stream.collect_all().await;
    assert_eq!(all.len(), 2);
}

// ===========================================================================
// 13. Replay (EventRecorder)
// ===========================================================================

#[tokio::test]
async fn replay_records_all() {
    let recorder = EventRecorder::new();
    recorder.record(&delta("a"));
    recorder.record(&error_event("e"));
    recorder.record(&run_completed());

    let events = recorder.events();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn replay_clear_and_rerecord() {
    let recorder = EventRecorder::new();
    recorder.record(&delta("a"));
    assert_eq!(recorder.len(), 1);

    recorder.clear();
    assert!(recorder.is_empty());

    recorder.record(&delta("b"));
    assert_eq!(recorder.len(), 1);
}

#[tokio::test]
async fn replay_through_pipeline() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();

    pipeline.process(delta("a"));
    pipeline.process(delta("b"));
    pipeline.process(error_event("e"));

    let events = pipeline.recorder().unwrap().events();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn replay_pipeline_records_only_passing_events() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(recorder.clone())
        .build();

    pipeline.process(delta("a"));
    pipeline.process(error_event("e"));
    pipeline.process(delta("b"));

    assert_eq!(recorder.len(), 2);
}

#[tokio::test]
async fn replay_collector_stream() {
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        let _ = tx.send(delta("a")).await;
        let _ = tx.send(delta("b")).await;
    });

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let handle = collector.collected();

    tokio::pin!(collector);
    let mut items = Vec::new();
    while let Some(item) = collector.next().await {
        items.push(item);
    }
    assert_eq!(items.len(), 2);

    let collected = handle.lock().unwrap();
    assert_eq!(collected.len(), 2);
}

#[tokio::test]
async fn replay_recorder_clone_is_shared() {
    let recorder = EventRecorder::new();
    let clone = recorder.clone();
    recorder.record(&delta("a"));
    assert_eq!(clone.len(), 1);
}

// ===========================================================================
// 14. Metrics Collection
// ===========================================================================

#[tokio::test]
async fn metrics_count_events() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("a"));
    metrics.record_event(&delta("b"));
    metrics.record_event(&error_event("e"));
    assert_eq!(metrics.event_count(), 3);
}

#[tokio::test]
async fn metrics_total_bytes_from_deltas() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("hello")); // 5 bytes
    metrics.record_event(&delta("world")); // 5 bytes
    metrics.record_event(&error_event("e")); // not delta, no bytes
    assert_eq!(metrics.total_bytes(), 10);
}

#[tokio::test]
async fn metrics_event_type_counts() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("a"));
    metrics.record_event(&delta("b"));
    metrics.record_event(&error_event("e"));
    metrics.record_event(&run_started());

    let counts = metrics.event_type_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("error"), Some(&1));
    assert_eq!(counts.get("run_started"), Some(&1));
}

#[tokio::test]
async fn metrics_summary_display() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("hello"));
    let summary = metrics.summary();
    let display = format!("{summary}");
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 5"));
}

#[tokio::test]
async fn metrics_throughput_zero_for_single_event() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("a"));
    // Single event → no time span → throughput is 0
    assert_eq!(metrics.throughput(), 0.0);
}

#[tokio::test]
async fn metrics_throughput_positive_for_spaced_events() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("a"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    metrics.record_event(&delta("b"));
    assert!(metrics.throughput() > 0.0);
}

#[tokio::test]
async fn event_stats_kind_counts() {
    let stats = EventStats::new();
    stats.observe(&delta("a"));
    stats.observe(&delta("b"));
    stats.observe(&tool_call("t", Some("id")));
    stats.observe(&warning_event("w"));

    assert_eq!(stats.count_for("assistant_delta"), 2);
    assert_eq!(stats.count_for("tool_call"), 1);
    assert_eq!(stats.count_for("warning"), 1);
    assert_eq!(stats.count_for("nonexistent"), 0);
}

#[tokio::test]
async fn event_stats_delta_bytes() {
    let stats = EventStats::new();
    stats.observe(&delta("abc")); // 3
    stats.observe(&delta("de")); // 2
    assert_eq!(stats.total_delta_bytes(), 5);
}

#[tokio::test]
async fn event_stats_reset() {
    let stats = EventStats::new();
    stats.observe(&delta("a"));
    stats.observe(&error_event("e"));
    assert_eq!(stats.total_events(), 2);

    stats.reset();
    assert_eq!(stats.total_events(), 0);
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.total_delta_bytes(), 0);
}

#[tokio::test]
async fn event_stats_in_pipeline() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    pipeline.process(delta("a"));
    pipeline.process(delta("b"));
    pipeline.process(error_event("e"));

    let pstats = pipeline.stats().unwrap();
    assert_eq!(pstats.total_events(), 3);
    assert_eq!(pstats.error_count(), 1);
}

// ===========================================================================
// Additional cross-cutting tests
// ===========================================================================

#[tokio::test]
async fn pipeline_filter_transform_record_stats_combo() {
    let stats = EventStats::new();
    let recorder = EventRecorder::new();

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .with_stats(stats.clone())
        .with_recorder(recorder.clone())
        .build();

    pipeline.process(delta("hello"));
    pipeline.process(error_event("skip"));
    pipeline.process(delta("world"));

    // 2 events pass through (errors filtered)
    assert_eq!(stats.total_events(), 2);
    assert_eq!(recorder.len(), 2);

    // Text should be uppercased
    let events = recorder.events();
    if let AgentEventKind::AssistantDelta { text } = &events[0].kind {
        assert_eq!(text, "HELLO");
    }
}

#[tokio::test]
async fn tee_all_receivers_closed() {
    let (tx1, rx1) = mpsc::channel(1);
    let (tx2, rx2) = mpsc::channel(1);
    let tee = StreamTee::new(vec![tx1, tx2]);

    drop(rx1);
    drop(rx2);

    let result = tee.send(&delta("orphan")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_active_count_tracks_drops() {
    let (tx1, rx1) = mpsc::channel(1);
    let (tx2, _rx2) = mpsc::channel(1);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);

    drop(rx1);
    // After dropping rx1, active count should decrease
    assert_eq!(tee.active_count(), 1);
}

#[tokio::test]
async fn aggregator_summary_no_events() {
    let agg = StreamAggregator::new();
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.text_length, 0);
    assert_eq!(summary.tool_call_count, 0);
    assert!(!summary.has_thinking);
    assert!(!summary.has_errors);
}

#[tokio::test]
async fn buffer_stream_ring_preserves_order() {
    let mut buf = StreamBuffer::new(5);
    for i in 0..10 {
        buf.push(delta(&format!("{i}")));
    }
    let recent = buf.recent(5);
    assert_eq!(recent.len(), 5);
    // Should contain 5..9
    for (idx, ev) in recent.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("{}", idx + 5));
        }
    }
}

#[tokio::test]
async fn full_lifecycle_stream() {
    let mut agg = StreamAggregator::new();
    let stats = EventStats::new();
    let recorder = EventRecorder::new();

    let events = vec![
        run_started(),
        delta("Hello"),
        delta(", "),
        delta("world!"),
        tool_call("search", Some("tc1")),
        tool_result("search", Some("tc1"), "found 3 results"),
        file_changed("src/main.rs"),
        command_executed("cargo build", 0),
        warning_event("deprecated API"),
        run_completed(),
    ];

    for ev in &events {
        agg.push(ev);
        stats.observe(ev);
        recorder.record(ev);
    }

    assert!(agg.is_complete());
    assert_eq!(agg.text(), "Hello, world!");
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(!agg.to_summary().has_errors);

    assert_eq!(stats.total_events(), 10);
    assert_eq!(stats.count_for("assistant_delta"), 3);
    assert_eq!(stats.count_for("tool_call"), 1);
    assert_eq!(stats.count_for("tool_result"), 1);

    assert_eq!(recorder.len(), 10);
}
