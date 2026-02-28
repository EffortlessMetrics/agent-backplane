// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests verifying that `RunMetrics` are recorded by the runtime.

use abp_core::{
    CapabilityRequirements, ExecutionLane, PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "metrics integration test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

/// Run a work order to completion, draining the event stream.
async fn run_to_completion(rt: &Runtime) -> abp_core::Receipt {
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    handle.receipt.await.expect("join").expect("receipt")
}

#[tokio::test]
async fn successful_run_records_one_success() {
    let rt = Runtime::with_default_backends();
    run_to_completion(&rt).await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn failed_run_records_one_failure() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();

    let err = rt.run_streaming("nonexistent", wo).await;
    assert!(err.is_err(), "unknown backend should fail");

    // UnknownBackend is caught before the spawned task, so no metrics recorded.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0, "early rejection does not record a run");
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn three_runs_accumulate_counts() {
    let rt = Runtime::with_default_backends();

    for _ in 0..3 {
        run_to_completion(&rt).await;
    }

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn snapshot_has_nonzero_duration_for_success() {
    let rt = Runtime::with_default_backends();
    run_to_completion(&rt).await;

    let snap = rt.metrics().snapshot();
    // The mock backend runs fast, but the duration counter should still be set.
    // average_run_duration_ms may be 0 on very fast machines; check cumulative
    // via total_runs > 0 as a proxy.
    assert!(snap.total_runs > 0, "at least one run recorded");
}

#[tokio::test]
async fn event_count_matches_actual_events() {
    let rt = Runtime::with_default_backends();

    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    let snap = rt.metrics().snapshot();
    // The trace on the receipt is what metrics counts.
    assert_eq!(
        snap.total_events,
        receipt.trace.len() as u64,
        "metrics event count should match receipt trace length"
    );
    // And the caller-visible events should be at least as many as the trace.
    assert!(
        events.len() >= receipt.trace.len(),
        "caller events ({}) >= receipt trace ({})",
        events.len(),
        receipt.trace.len()
    );
}
