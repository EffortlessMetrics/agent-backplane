// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests using the mock backend.
//!
//! These tests exercise the full pipeline: Runtime -> Backend -> Receipt

use abp_core::{
    AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement, CapabilityRequirements,
    ExecutionLane, MinSupport, Outcome, PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use std::collections::HashSet;
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "e2e test task".into(),
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

// ---------- 1. Full mock pipeline ----------

#[tokio::test]
async fn full_mock_pipeline() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let wo_id = wo.id;

    let (events, receipt) = run_to_completion(&rt, wo).await;

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(matches!(receipt.outcome, Outcome::Complete));
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt must have sha256 hash"
    );

    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_started, "events must include RunStarted");
    assert!(has_completed, "events must include RunCompleted");
}

// ---------- 2. Policy enforcement with mock ----------

#[tokio::test]
async fn policy_enforcement_compiles() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into(), "/etc/passwd".into()],
        ..Default::default()
    };

    // The runtime compiles the policy even if mock doesn't enforce it.
    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ---------- 3. Staged workspace with mock ----------

#[tokio::test]
async fn staged_workspace_has_git_metadata() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::fs::write(tmp.path().join("hello.txt"), "world").expect("write file");

    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.workspace = WorkspaceSpec {
        root: tmp.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };

    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
    // Staged mode initializes a git repo, so these should be populated.
    assert!(
        receipt.verification.git_status.is_some(),
        "staged workspace should have git_status"
    );
}

// ---------- 4. Multiple sequential runs ----------

#[tokio::test]
async fn multiple_sequential_runs_unique_ids() {
    let rt = Runtime::with_default_backends();
    let mut run_ids = HashSet::new();

    for _ in 0..3 {
        let handle = rt
            .run_streaming("mock", mock_work_order())
            .await
            .expect("run_streaming");
        run_ids.insert(handle.run_id);
        let _: Vec<_> = handle.events.collect().await;
        let _ = handle.receipt.await;
    }

    assert_eq!(run_ids.len(), 3, "each run must have a unique run_id");
}

// ---------- 5. Satisfiable capability requirements ----------

#[tokio::test]
async fn satisfiable_capability_requirements() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ---------- 6. Unsatisfiable capability requirements ----------

#[tokio::test]
async fn unsatisfiable_capability_requirements() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    // The capability pre-check now happens inside run_streaming, so it returns an error directly.
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err(), "unsatisfiable requirements should fail");
    let err_msg = result.err().map(|e| format!("{e}")).unwrap_or_default();
    assert!(
        err_msg.contains("capability")
            || err_msg.contains("unsatisfied")
            || err_msg.contains("Capability"),
        "error should mention capability/backend issue: {err_msg}"
    );
}

// ---------- 7. Receipt hash verification ----------

#[tokio::test]
async fn receipt_hash_verification() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;

    let stored_hash = receipt.receipt_sha256.clone().expect("hash must exist");
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(
        stored_hash, recomputed,
        "stored hash must match recomputed hash"
    );
}

// ---------- 8. Event ordering ----------

#[tokio::test]
async fn event_ordering_started_before_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    let started_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .expect("RunStarted must be present");
    let completed_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .expect("RunCompleted must be present");

    assert!(
        started_idx < completed_idx,
        "RunStarted (idx {started_idx}) must come before RunCompleted (idx {completed_idx})"
    );
}
