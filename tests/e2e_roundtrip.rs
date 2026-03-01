// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end roundtrip tests verifying the full sidecar path through the runtime.
//!
//! Each test uses the real `Runtime` type with `MockBackend` (no real API calls).

use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement,
    CapabilityRequirements, MinSupport, Outcome, PolicyProfile, Receipt, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, chain::ReceiptChain, receipt_hash,
};
use abp_runtime::{Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a RunHandle.
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

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

// ---------------------------------------------------------------------------
// 1. Full roundtrip: work order → runtime → receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_backend_full_roundtrip() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("roundtrip test");
    let wo_id = wo.id;

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.expect("receipt should be Ok");

    // Receipt metadata
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");

    // Events were emitted
    assert!(!events.is_empty(), "expected at least one event");
}

// ---------------------------------------------------------------------------
// 2. Events are streamed correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_backend_streams_events() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("event streaming test");

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.expect("receipt");

    // MockBackend emits: RunStarted, 2x AssistantMessage, RunCompleted
    assert!(
        events.len() >= 4,
        "expected >= 4 events, got {}",
        events.len()
    );

    // First event should be RunStarted
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got {:?}",
        events[0].kind
    );

    // Last event should be RunCompleted
    assert!(
        matches!(
            &events[events.len() - 1].kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );

    // Receipt trace should also contain the events
    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should not be empty"
    );
}

// ---------------------------------------------------------------------------
// 3. Receipt hash integrity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_backend_receipt_hashing() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash test");

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.expect("receipt");

    // Hash should be present
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt_sha256 should be set"
    );

    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest should be 64 chars");

    // Recompute and verify
    let recomputed = receipt_hash(&receipt).expect("receipt_hash");
    assert_eq!(
        hash, &recomputed,
        "stored hash should match recomputed hash"
    );
}

// ---------------------------------------------------------------------------
// 4. Workspace staging roundtrip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_workspace_staging_roundtrip() {
    let rt = Runtime::with_default_backends();

    // Create a temp directory with a test file so staging has something to copy.
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::fs::write(tmp.path().join("hello.txt"), "world").expect("write test file");

    let wo = WorkOrderBuilder::new("workspace staging test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(tmp.path().to_string_lossy().as_ref())
        .build();

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.expect("receipt");

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());

    // The staged workspace should have been cleaned up (temp dir dropped).
    // We can't directly check this, but the run completing without error
    // proves the staging path worked.
}

// ---------------------------------------------------------------------------
// 5. Policy enforcement blocks tool
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_policy_enforcement_blocks_tool() {
    // Policy enforcement in v0.1 is best-effort at the runtime level.
    // The runtime compiles the policy, but tool blocking is up to backends.
    // Here we verify that the runtime at least compiles restrictive policies
    // without errors and produces a valid receipt.

    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("policy test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.expect("receipt");

    // The policy was compiled and the run completed.
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());

    // Verify the policy engine can independently check tool access.
    let engine = abp_policy::PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    })
    .unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

// ---------------------------------------------------------------------------
// 6. Capability checking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_capability_checking() {
    let rt = Runtime::with_default_backends();

    // MockBackend supports Streaming (native). This should pass.
    let reqs_ok = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    rt.check_capabilities("mock", &reqs_ok)
        .expect("streaming should be satisfied");

    // MockBackend does NOT support McpClient. Pre-flight check should fail.
    let reqs_fail = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs_fail).unwrap_err();
    assert!(
        matches!(err, RuntimeError::CapabilityCheckFailed(_)),
        "expected CapabilityCheckFailed, got {err:?}"
    );

    // Unknown backend should return UnknownBackend.
    let err = rt.check_capabilities("nonexistent", &reqs_ok).unwrap_err();
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );

    // run_streaming should also reject an unsatisfied requirement.
    let wo = WorkOrderBuilder::new("cap check test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs_fail.clone())
        .build();
    let err = rt.run_streaming("mock", wo).await.err();
    assert!(
        matches!(err, Some(RuntimeError::CapabilityCheckFailed(_))),
        "run_streaming should reject unsatisfied caps, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 7. Multiple sequential runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multiple_sequential_runs() {
    let rt = Runtime::with_default_backends();

    let mut receipts = Vec::new();
    for i in 0..3 {
        let wo = simple_work_order(&format!("sequential run {i}"));
        let wo_id = wo.id;

        let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
        let (_events, receipt) = drain_run(handle).await;
        let receipt = receipt.expect("receipt");

        assert_eq!(receipt.meta.work_order_id, wo_id);
        assert_eq!(receipt.outcome, Outcome::Complete);
        receipts.push(receipt);
    }

    // Each run should have a unique run_id.
    let run_ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(run_ids.len(), 3, "each run should have a unique run_id");

    // Each receipt should have a valid hash.
    for r in &receipts {
        let hash = r.receipt_sha256.as_ref().expect("hash should be set");
        let recomputed = receipt_hash(r).expect("receipt_hash");
        assert_eq!(hash, &recomputed);
    }
}

// ---------------------------------------------------------------------------
// 8. Concurrent runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_concurrent_runs() {
    let rt = Runtime::with_default_backends();

    let mut handles = Vec::new();
    let mut work_order_ids = Vec::new();

    for i in 0..5 {
        let wo = simple_work_order(&format!("concurrent run {i}"));
        work_order_ids.push(wo.id);
        let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
        handles.push(handle);
    }

    let mut receipts = Vec::new();
    for handle in handles {
        let (_events, receipt) = drain_run(handle).await;
        receipts.push(receipt.expect("receipt"));
    }

    assert_eq!(receipts.len(), 5);

    // All runs should complete successfully.
    for (receipt, wo_id) in receipts.iter().zip(work_order_ids.iter()) {
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.meta.work_order_id, *wo_id);
        assert!(receipt.receipt_sha256.is_some());
    }

    // All run_ids should be unique.
    let run_ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(run_ids.len(), 5, "concurrent runs should have unique IDs");
}

// ---------------------------------------------------------------------------
// 9. Event ordering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_event_ordering() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("event ordering test");

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let (events, _receipt) = drain_run(handle).await;

    // Timestamps should be monotonically non-decreasing.
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "events should be temporally ordered: {:?} should come after {:?}",
            window[1].ts,
            window[0].ts,
        );
    }

    // RunStarted should come first, RunCompleted should come last.
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted"
    );
    assert!(
        matches!(
            &events[events.len() - 1].kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
}

// ---------------------------------------------------------------------------
// 10. Receipt chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_receipt_chain() {
    let rt = Runtime::with_default_backends();
    let mut chain = ReceiptChain::new();

    for i in 0..3 {
        let wo = simple_work_order(&format!("chain run {i}"));
        let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
        let (_events, receipt) = drain_run(handle).await;
        let receipt = receipt.expect("receipt");

        chain.push(receipt).expect("chain push should succeed");
    }

    assert_eq!(chain.len(), 3);
    assert!(!chain.is_empty());

    // Full chain verification should pass.
    chain.verify().expect("chain verify");

    // All receipts in the chain should be Complete.
    assert!(
        (chain.success_rate() - 1.0).abs() < f64::EPSILON,
        "all receipts should be Complete"
    );

    // Total events should be > 0.
    assert!(chain.total_events() > 0, "chain should have events");

    // Each receipt should link to a distinct run_id.
    let ids: std::collections::HashSet<_> = chain.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 3);
}
