// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming semantics tests: event ordering, channel lifecycle, and payload integrity.
//!
//! 10 tests verifying that the ABP event stream preserves ordering, handles
//! channel closure correctly, and delivers every event faithfully.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a [`RunHandle`].
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("backend task panicked");
    (collected, receipt)
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
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
            id: "streaming-test".into(),
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
// Custom backends for streaming scenarios
// ---------------------------------------------------------------------------

/// Backend that emits numbered AssistantDelta events for ordering verification.
#[derive(Debug, Clone)]
struct OrderedDeltaBackend {
    count: usize,
}

#[async_trait]
impl Backend for OrderedDeltaBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "ordered-delta".into(),
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
                message: "start".into(),
            },
        )
        .await;
        for i in 0..self.count {
            emit(
                &mut trace,
                &tx,
                AgentEventKind::AssistantDelta {
                    text: format!("delta-{i}"),
                },
            )
            .await;
        }
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

/// Backend that emits a mixed sequence of event types.
#[derive(Debug, Clone)]
struct MixedEventBackend;

#[async_trait]
impl Backend for MixedEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "mixed-events".into(),
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
                message: "start".into(),
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
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "main.rs"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
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

/// Backend that emits an error event mid-stream, then continues with more events.
#[derive(Debug, Clone)]
struct ErrorMidStreamBackend;

#[async_trait]
impl Backend for ErrorMidStreamBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-midstream".into(),
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
                message: "start".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "before error".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Error {
                message: "transient failure".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "after error".into(),
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

/// Backend that emits zero events (no events between start and end).
#[derive(Debug, Clone)]
struct EmptyStreamBackend;

#[async_trait]
impl Backend for EmptyStreamBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "empty-stream".into(),
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
                message: "start".into(),
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

fn simple_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

// ===========================================================================
// 1. Event ordering preservation
// ===========================================================================

#[tokio::test]
async fn event_ordering_preserved_across_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 20 });

    let handle = rt
        .run_streaming("ordered", simple_wo("ordering"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    // First event is RunStarted, last is RunCompleted.
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[events.len() - 1].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    // Deltas appear in order 0..19 between start and end.
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 20);
    for (i, txt) in deltas.iter().enumerate() {
        assert_eq!(*txt, format!("delta-{i}"));
    }
}

// ===========================================================================
// 2. Multiple event types in sequence
// ===========================================================================

#[tokio::test]
async fn multiple_event_types_in_sequence() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", MixedEventBackend);

    let handle = rt
        .run_streaming("mixed", simple_wo("mixed types"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    // Expected: RunStarted, AssistantDelta, ToolCall, ToolResult, AssistantDelta, RunCompleted
    let type_tags: Vec<&str> = events
        .iter()
        .map(|e| match &e.kind {
            AgentEventKind::RunStarted { .. } => "run_started",
            AgentEventKind::AssistantDelta { .. } => "assistant_delta",
            AgentEventKind::ToolCall { .. } => "tool_call",
            AgentEventKind::ToolResult { .. } => "tool_result",
            AgentEventKind::RunCompleted { .. } => "run_completed",
            _ => "other",
        })
        .collect();

    assert_eq!(
        type_tags,
        vec![
            "run_started",
            "assistant_delta",
            "tool_call",
            "tool_result",
            "assistant_delta",
            "run_completed",
        ]
    );
}

// ===========================================================================
// 3. Channel closure semantics (sender drop → receiver gets None)
// ===========================================================================

#[tokio::test]
async fn channel_closure_after_sender_drop() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);

    // Send one event, then drop sender.
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "only".into(),
    }))
    .await
    .unwrap();
    drop(tx);

    // Receiver should get the event, then None.
    let first = rx.recv().await;
    assert!(first.is_some());
    let second = rx.recv().await;
    assert!(
        second.is_none(),
        "receiver should yield None after sender drop"
    );
}

// ===========================================================================
// 4. Error events don't terminate the stream
// ===========================================================================

#[tokio::test]
async fn error_event_does_not_terminate_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("error-mid", ErrorMidStreamBackend);

    let handle = rt
        .run_streaming("error-mid", simple_wo("error mid"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Find the error event index.
    let error_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .expect("error event should be present");

    // Events continue after the error.
    assert!(
        events.len() > error_idx + 1,
        "events should continue after error event"
    );

    // Verify we got the post-error delta.
    let post_error = &events[error_idx + 1];
    assert!(
        matches!(&post_error.kind, AgentEventKind::AssistantDelta { text } if text == "after error"),
        "expected delta after error, got {:?}",
        post_error.kind
    );

    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 5. Large payload streaming (many events, verify all arrive)
// ===========================================================================

#[tokio::test]
async fn large_payload_streaming_all_events_arrive() {
    let count = 1000;
    let mut rt = Runtime::new();
    rt.register_backend("large", OrderedDeltaBackend { count });

    let handle = rt
        .run_streaming("large", simple_wo("large stream"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        deltas.len(),
        count,
        "all {count} delta events should arrive"
    );

    // Spot-check first, middle, last.
    assert_eq!(deltas[0], "delta-0");
    assert_eq!(deltas[count / 2], format!("delta-{}", count / 2));
    assert_eq!(deltas[count - 1], format!("delta-{}", count - 1));
}

// ===========================================================================
// 6. Empty event stream (only start/end, no payload events)
// ===========================================================================

#[tokio::test]
async fn empty_event_stream_produces_valid_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("empty", EmptyStreamBackend);

    let handle = rt
        .run_streaming("empty", simple_wo("empty stream"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Only RunStarted and RunCompleted.
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
}

// ===========================================================================
// 7. Event metadata consistency (timestamps are monotonically non-decreasing)
// ===========================================================================

#[tokio::test]
async fn event_timestamps_non_decreasing() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 50 });

    let handle = rt
        .run_streaming("ordered", simple_wo("timestamps"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "timestamps must be non-decreasing: {:?} followed {:?}",
            window[1].ts,
            window[0].ts,
        );
    }
}

// ===========================================================================
// 8. Concurrent sender/receiver (sender sends while receiver reads)
// ===========================================================================

#[tokio::test]
async fn concurrent_sender_receiver() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(4); // small buffer to create backpressure
    let count = 100;

    let sender = tokio::spawn(async move {
        for i in 0..count {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("msg-{i}"),
            }))
            .await
            .unwrap();
        }
        // Drop tx to signal completion.
    });

    let receiver = tokio::spawn(async move {
        let mut stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let mut collected = Vec::new();
        while let Some(ev) = stream.next().await {
            collected.push(ev);
        }
        collected
    });

    sender.await.unwrap();
    let received = receiver.await.unwrap();

    assert_eq!(received.len(), count);
    // Verify ordering preserved even under concurrent pressure.
    for (i, ev) in received.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert_eq!(text, &format!("msg-{i}"));
            }
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }
}

// ===========================================================================
// 9. Back-to-back text deltas (all preserved, not merged)
// ===========================================================================

#[tokio::test]
async fn back_to_back_deltas_all_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 5 });

    let handle = rt
        .run_streaming("ordered", simple_wo("back-to-back"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    // All 5 separate deltas preserved (not merged into one).
    assert_eq!(deltas.len(), 5);
    assert_eq!(
        deltas,
        vec!["delta-0", "delta-1", "delta-2", "delta-3", "delta-4"]
    );
}

// ===========================================================================
// 10. Event count matches between send and receive
// ===========================================================================

#[tokio::test]
async fn event_count_matches_send_and_receive() {
    let event_count = 10;
    let mut rt = Runtime::new();
    rt.register_backend("counted", OrderedDeltaBackend { count: event_count });

    let handle = rt
        .run_streaming("counted", simple_wo("count match"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Backend emits: 1 RunStarted + event_count deltas + 1 RunCompleted
    let expected_total = 1 + event_count + 1;
    assert_eq!(
        events.len(),
        expected_total,
        "streamed event count should match: expected {expected_total}, got {}",
        events.len()
    );

    // Receipt trace should also contain the same count.
    assert_eq!(
        receipt.trace.len(),
        expected_total,
        "receipt trace count should match streamed events"
    );
}
