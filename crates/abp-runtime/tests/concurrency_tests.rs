// SPDX-License-Identifier: MIT OR Apache-2.0
//! Concurrency and stress tests for the ABP runtime.
//!
//! All tests use [`MockBackend`] and avoid wall-clock timing assertions
//! so they remain deterministic on slow CI runners.

use std::collections::HashSet;
use std::sync::Arc;

use abp_core::{
    ExecutionLane, Outcome, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::Runtime;
use tokio_stream::StreamExt;

/// Build a minimal work order suitable for mock-backend tests.
fn mock_work_order(task: &str) -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: task.into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: abp_core::PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

/// Drive a run to completion, returning the receipt.
async fn run_to_receipt(
    rt: &Runtime,
    wo: WorkOrder,
) -> abp_core::Receipt {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    // Drain the event stream so the run can finish.
    let _: Vec<_> = handle.events.collect().await;
    handle.receipt.await.expect("join").expect("receipt")
}

// ---------------------------------------------------------------------------
// 1. Sequential runs — 10 work orders, all complete with unique IDs
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn sequential_runs_all_complete_with_unique_ids() {
    let rt = Runtime::with_default_backends();
    let mut run_ids = HashSet::new();

    for i in 0..10 {
        let wo = mock_work_order(&format!("seq-{i}"));
        let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
        run_ids.insert(handle.run_id);
        let _: Vec<_> = handle.events.collect().await;
        let receipt = handle.receipt.await.expect("join").expect("receipt");
        assert!(
            matches!(receipt.outcome, Outcome::Complete),
            "run {i} should complete successfully"
        );
        assert!(
            receipt.receipt_sha256.is_some(),
            "run {i} must have a receipt hash"
        );
    }

    assert_eq!(run_ids.len(), 10, "all 10 runs must have unique run_ids");
}

// ---------------------------------------------------------------------------
// 2. Rapid fire — submit many runs in quick succession (concurrently)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn rapid_fire_concurrent_runs() {
    let rt = Arc::new(Runtime::with_default_backends());
    let count = 20;

    let mut handles = Vec::with_capacity(count);
    for i in 0..count {
        let rt = Arc::clone(&rt);
        handles.push(tokio::spawn(async move {
            let wo = mock_work_order(&format!("rapid-{i}"));
            let run_handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
            let run_id = run_handle.run_id;
            let _: Vec<_> = run_handle.events.collect().await;
            let receipt = run_handle.receipt.await.expect("join").expect("receipt");
            (run_id, receipt)
        }));
    }

    let mut run_ids = HashSet::new();
    for h in handles {
        let (run_id, receipt) = h.await.expect("task join");
        run_ids.insert(run_id);
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(receipt.receipt_sha256.is_some());
    }

    assert_eq!(run_ids.len(), count, "every concurrent run must have a unique id");
}

// ---------------------------------------------------------------------------
// 3. Run after error — a failed run must not poison subsequent runs
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn run_after_error_succeeds() {
    let rt = Runtime::with_default_backends();

    // Trigger an error by requesting an unknown backend.
    let err = rt
        .run_streaming("does-not-exist", mock_work_order("fail"))
        .await;
    assert!(err.is_err(), "unknown backend should fail");

    // A subsequent valid run must still succeed.
    let receipt = run_to_receipt(&rt, mock_work_order("after-error")).await;
    assert!(
        matches!(receipt.outcome, Outcome::Complete),
        "run after error should complete"
    );
    assert!(receipt.receipt_sha256.is_some());
}

// ---------------------------------------------------------------------------
// 4. Telemetry under load — record many runs, verify metric invariants
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn telemetry_consistent_under_concurrent_records() {
    let metrics = Arc::new(RunMetrics::new());
    let total = 50u64;

    let mut handles = Vec::with_capacity(total as usize);
    for i in 0..total {
        let m = Arc::clone(&metrics);
        handles.push(tokio::spawn(async move {
            let success = i % 3 != 0; // ~2/3 succeed
            m.record_run(10, success, 2);
        }));
    }

    for h in handles {
        h.await.expect("task join");
    }

    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, total, "total_runs must equal iterations");
    assert_eq!(
        snap.successful_runs + snap.failed_runs,
        total,
        "success + failure must equal total"
    );
    assert_eq!(
        snap.total_events,
        total * 2,
        "each run records 2 events"
    );
    assert_eq!(
        snap.average_run_duration_ms, 10,
        "all durations are 10 ms so average must be 10"
    );
}

// ---------------------------------------------------------------------------
// 5. Registry thread safety — read registry from multiple tasks while running
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn registry_reads_safe_during_run() {
    let rt = Arc::new(Runtime::with_default_backends());

    // Start a run in the background.
    let rt2 = Arc::clone(&rt);
    let run_task = tokio::spawn(async move {
        run_to_receipt(&rt2, mock_work_order("background")).await
    });

    // Concurrently read from the registry many times.
    let mut readers = Vec::new();
    for _ in 0..20 {
        let rt_ref = Arc::clone(&rt);
        readers.push(tokio::spawn(async move {
            let names = rt_ref.backend_names();
            assert!(names.contains(&"mock".to_string()), "mock must be listed");
            let backend = rt_ref.backend("mock");
            assert!(backend.is_some(), "mock backend must be retrievable");
            let reg = rt_ref.registry();
            assert!(reg.contains("mock"), "registry must contain mock");
        }));
    }

    for r in readers {
        r.await.expect("reader task join");
    }

    let receipt = run_task.await.expect("run task join");
    assert!(matches!(receipt.outcome, Outcome::Complete));
}
