// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests verifying that `abp-stream` primitives (EventFilter,
//! EventTransform, EventMultiplexer, StreamPipeline) work correctly with
//! runtime-produced agent events.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::Runtime;
use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipelineBuilder,
};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

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

fn tool_call_event(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

async fn emit(trace: &mut Vec<AgentEvent>, tx: &mpsc::Sender<AgentEvent>, kind: AgentEventKind) {
    let ev = make_event(kind);
    trace.push(ev.clone());
    let _ = tx.send(ev).await;
}

fn build_receipt(
    run_id: Uuid,
    work_order: &WorkOrder,
    trace: Vec<AgentEvent>,
    started: chrono::DateTime<Utc>,
) -> anyhow::Result<Receipt> {
    let finished = Utc::now();
    let receipt = Receipt {
        meta: abp_core::RunMetadata {
            run_id,
            work_order_id: work_order.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
        },
        backend: BackendIdentity {
            id: "stream-test".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: Default::default(),
        trace,
        artifacts: vec![],
        verification: Default::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
}

// ---------------------------------------------------------------------------
// Custom backend that emits a known sequence of events
// ---------------------------------------------------------------------------

/// Backend that emits deltas, tool calls, errors, and lifecycle events.
#[derive(Debug, Clone)]
struct MixedEventBackend;

#[async_trait]
impl Backend for MixedEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "mixed-stream".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "hello ".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Error {
                message: "simulated error".into(),
                error_code: None,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "world".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started)
    }
}

// ---------------------------------------------------------------------------
// Tests: Stream filtering with runtime events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filter_errors_from_runtime_stream() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", make_work_order("filter test"))
        .await
        .unwrap();

    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.unwrap().unwrap();

    // The backend emits 6 events (RunStarted, Delta, ToolCall, Error, Delta, RunCompleted).
    // The pipeline filters out the Error, so the caller sees 5.
    assert_eq!(
        collected.len(),
        5,
        "expected 5 events after filtering errors"
    );
    assert!(
        !collected
            .iter()
            .any(|ev| matches!(ev.kind, AgentEventKind::Error { .. })),
        "no error events should pass the filter"
    );
    // The backend's own receipt trace is unfiltered (6 events) because the
    // backend populates its trace before the runtime pipeline runs.
    assert_eq!(receipt.trace.len(), 6);
}

#[tokio::test]
async fn filter_by_kind_keeps_only_deltas() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();

    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", make_work_order("delta filter"))
        .await
        .unwrap();

    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let _ = handle.receipt.await.unwrap();

    assert_eq!(collected.len(), 2, "only 2 delta events expected");
    for ev in &collected {
        assert!(
            matches!(ev.kind, AgentEventKind::AssistantDelta { .. }),
            "all events should be assistant deltas"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: Stream transformation with runtime events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transform_adds_metadata_to_runtime_events() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("pipeline".to_string(), serde_json::json!("stream-v1"));
            ev
        }))
        .build();

    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", make_work_order("transform test"))
        .await
        .unwrap();

    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let _ = handle.receipt.await.unwrap();

    assert_eq!(collected.len(), 6);
    for ev in &collected {
        let ext = ev.ext.as_ref().expect("ext should be set by transform");
        assert_eq!(
            ext.get("pipeline").unwrap(),
            &serde_json::json!("stream-v1")
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: Pipeline with recorder and stats
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_records_and_tracks_stats_in_runtime() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", make_work_order("stats test"))
        .await
        .unwrap();

    let mut events = handle.events;
    while events.next().await.is_some() {}
    let _ = handle.receipt.await.unwrap();

    // 6 events emitted, 1 error filtered out = 5 recorded
    assert_eq!(recorder.len(), 5);
    assert_eq!(stats.total_events(), 5);
    assert_eq!(stats.error_count(), 0, "errors filtered before stats");
    assert_eq!(stats.count_for("assistant_delta"), 2);
    assert_eq!(stats.count_for("tool_call"), 1);
}

// ---------------------------------------------------------------------------
// Tests: Standalone EventStream with filtering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_stream_collect_filtered_integration() {
    let (tx, rx) = mpsc::channel(32);

    // Simulate a backend emitting events into a channel
    tokio::spawn(async move {
        tx.send(delta_event("a")).await.unwrap();
        tx.send(error_event("oops")).await.unwrap();
        tx.send(tool_call_event("write")).await.unwrap();
        tx.send(delta_event("b")).await.unwrap();
        drop(tx);
    });

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let events = stream.collect_filtered(&filter).await;

    assert_eq!(events.len(), 2);
}

// ---------------------------------------------------------------------------
// Tests: EventStream pipe through pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_stream_pipe_filters_and_transforms() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("piped".to_string(), serde_json::json!(true));
            ev
        }))
        .with_recorder(recorder.clone())
        .build();

    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    tokio::spawn(async move {
        tx_in.send(delta_event("hello")).await.unwrap();
        tx_in.send(error_event("bad")).await.unwrap();
        tx_in.send(tool_call_event("search")).await.unwrap();
        drop(tx_in);
    });

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }

    assert_eq!(results.len(), 2, "error should be filtered");
    assert_eq!(recorder.len(), 2);
    for ev in &results {
        assert_eq!(
            ev.ext.as_ref().unwrap().get("piped").unwrap(),
            &serde_json::json!(true)
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: EventMultiplexer merge with runtime-style events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiplexer_merges_two_backend_streams_sorted() {
    let ts_base = Utc::now();
    let ts1 = ts_base;
    let ts2 = ts_base + chrono::Duration::milliseconds(10);
    let ts3 = ts_base + chrono::Duration::milliseconds(20);
    let ts4 = ts_base + chrono::Duration::milliseconds(30);

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    // Stream 1: ts1, ts3
    tx1.send(AgentEvent {
        ts: ts1,
        kind: AgentEventKind::RunStarted {
            message: "backend-a".into(),
        },
        ext: None,
    })
    .await
    .unwrap();
    tx1.send(AgentEvent {
        ts: ts3,
        kind: AgentEventKind::AssistantDelta {
            text: "from-a".into(),
        },
        ext: None,
    })
    .await
    .unwrap();
    drop(tx1);

    // Stream 2: ts2, ts4
    tx2.send(AgentEvent {
        ts: ts2,
        kind: AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    })
    .await
    .unwrap();
    tx2.send(AgentEvent {
        ts: ts4,
        kind: AgentEventKind::RunCompleted {
            message: "backend-b".into(),
        },
        ext: None,
    })
    .await
    .unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let sorted = mux.collect_sorted().await;

    assert_eq!(sorted.len(), 4);
    assert_eq!(sorted[0].ts, ts1);
    assert_eq!(sorted[1].ts, ts2);
    assert_eq!(sorted[2].ts, ts3);
    assert_eq!(sorted[3].ts, ts4);
}

#[tokio::test]
async fn multiplexer_merge_channel_preserves_order() {
    let ts_base = Utc::now();

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(AgentEvent {
        ts: ts_base,
        kind: AgentEventKind::AssistantDelta {
            text: "first".into(),
        },
        ext: None,
    })
    .await
    .unwrap();

    tx2.send(AgentEvent {
        ts: ts_base + chrono::Duration::milliseconds(5),
        kind: AgentEventKind::AssistantDelta {
            text: "second".into(),
        },
        ext: None,
    })
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged_rx = mux.merge(16);

    let first = merged_rx.recv().await.unwrap();
    let second = merged_rx.recv().await.unwrap();
    assert!(first.ts <= second.ts, "events should be time-ordered");
    assert!(merged_rx.recv().await.is_none());
}

// ---------------------------------------------------------------------------
// Tests: Combined filter + transform + multiplex
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_filter_transform_with_multiplexed_streams() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("mux_processed".to_string(), serde_json::json!(true));
            ev
        }))
        .with_stats(stats.clone())
        .build();

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(delta_event("a")).await.unwrap();
    tx1.send(error_event("err")).await.unwrap();
    drop(tx1);

    tx2.send(tool_call_event("edit")).await.unwrap();
    tx2.send(delta_event("b")).await.unwrap();
    drop(tx2);

    // Merge two streams
    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let merged = mux.collect_sorted().await;

    // Process through pipeline
    let mut results = Vec::new();
    for ev in merged {
        if let Some(ev) = pipeline.process(ev) {
            results.push(ev);
        }
    }

    // 4 events, 1 error filtered = 3
    assert_eq!(results.len(), 3);
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.error_count(), 0);
    for ev in &results {
        assert_eq!(
            ev.ext.as_ref().unwrap().get("mux_processed").unwrap(),
            &serde_json::json!(true)
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: Runtime without pipeline (passthrough)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runtime_without_pipeline_passes_all_events() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", make_work_order("no pipeline"))
        .await
        .unwrap();

    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.unwrap().unwrap();

    // All 6 events pass through when no pipeline is set
    assert_eq!(collected.len(), 6);
    assert_eq!(receipt.trace.len(), 6);
}

// ---------------------------------------------------------------------------
// Tests: Runtime stream_pipeline accessor
// ---------------------------------------------------------------------------

#[test]
fn runtime_stream_pipeline_accessor() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    let rt = rt.with_stream_pipeline(pipeline);
    assert!(rt.stream_pipeline().is_some());
}
