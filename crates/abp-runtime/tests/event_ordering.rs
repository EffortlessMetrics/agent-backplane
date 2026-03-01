// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for event ordering and stream termination guarantees.
//!
//! These tests verify that the Runtime + MockBackend pipeline delivers events
//! in the correct order and that the stream terminates cleanly.

use abp_core::{
    AgentEventKind, ExecutionLane, Outcome, PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "event ordering test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

/// Helper: run a work order end-to-end, collecting events and receipt.
async fn run_to_completion(
    rt: &Runtime,
    wo: WorkOrder,
) -> (Vec<abp_core::AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ---------- 1. Event timestamps are non-decreasing ----------

#[tokio::test]
async fn event_timestamps_are_non_decreasing() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    assert!(events.len() >= 2, "expected at least 2 events");

    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "event timestamps must be non-decreasing: {:?} came after {:?}",
            window[1].ts,
            window[0].ts,
        );
    }
}

// ---------- 2. Events arrive in submission order ----------

#[tokio::test]
async fn events_arrive_in_submission_order() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    // MockBackend emits: RunStarted, AssistantMessage, AssistantMessage, RunCompleted
    assert_eq!(events.len(), 4, "mock backend should emit exactly 4 events");

    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event must be RunStarted, got {:?}",
        events[0].kind
    );
    assert!(
        matches!(&events[1].kind, AgentEventKind::AssistantMessage { .. }),
        "second event must be AssistantMessage, got {:?}",
        events[1].kind
    );
    assert!(
        matches!(&events[2].kind, AgentEventKind::AssistantMessage { .. }),
        "third event must be AssistantMessage, got {:?}",
        events[2].kind
    );
    assert!(
        matches!(&events[3].kind, AgentEventKind::RunCompleted { .. }),
        "last event must be RunCompleted, got {:?}",
        events[3].kind
    );
}

// ---------- 3. Stream terminates after receipt ----------

#[tokio::test]
async fn stream_terminates_after_receipt() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");

    // Drain all events first so the backend can finish.
    let events: Vec<_> = handle.events.collect().await;
    assert!(!events.is_empty(), "should have received events");

    // Receipt should now be available (backend is done).
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));

    // The stream already returned None (collect drained it), proving termination.
    // Re-wrapping a closed receiver also yields no items.
}

// ---------- 4. Empty task produces valid receipt ----------

#[tokio::test]
async fn empty_task_produces_valid_receipt() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.task = String::new();
    let wo_id = wo.id;

    let (_events, receipt) = run_to_completion(&rt, wo).await;

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert!(matches!(receipt.outcome, Outcome::Complete));
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt must have sha256 hash even for empty task"
    );

    // Hash must be valid (recompute and compare).
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(stored, recomputed, "hash must be self-consistent");
}

// ---------- 5. Multiple events collect correctly ----------

#[tokio::test]
async fn multiple_events_collect_correctly() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_to_completion(&rt, mock_work_order()).await;

    // MockBackend emits exactly 4 events.
    assert_eq!(
        events.len(),
        4,
        "mock backend should produce exactly 4 events"
    );

    // The receipt trace should also contain the same events.
    assert_eq!(
        receipt.trace.len(),
        events.len(),
        "receipt trace length must match streamed event count"
    );
}

// ---------- 6. Run ID is consistent across handle and receipt ----------

#[tokio::test]
async fn run_id_consistent_across_handle_and_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let wo_id = wo.id;

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let handle_run_id = handle.run_id;

    let _events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    // The run_id on the handle must match the receipt's run_id.
    assert_eq!(
        handle_run_id, receipt.meta.run_id,
        "RunHandle.run_id must match receipt.meta.run_id"
    );

    // The work order id must also be preserved.
    assert_eq!(
        wo_id, receipt.meta.work_order_id,
        "work_order_id must be preserved in receipt"
    );
}
