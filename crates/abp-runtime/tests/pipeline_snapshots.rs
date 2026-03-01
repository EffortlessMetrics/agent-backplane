// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests capturing the full pipeline output from a mock backend run.

use abp_core::{
    AgentEvent, CapabilityRequirements, ExecutionLane, PolicyProfile, WorkOrder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_runtime::{Runtime, RuntimeError};
use insta::{assert_json_snapshot, assert_snapshot};
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::nil(),
        task: "snapshot test task".into(),
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

async fn run_to_completion(rt: &Runtime, wo: WorkOrder) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ---------------------------------------------------------------------------
// 1. Receipt snapshot from a mock backend run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_mock_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;

    let value = serde_json::to_value(&receipt).unwrap();
    assert_json_snapshot!("pipeline_mock_receipt", value, {
        ".meta.run_id" => "[uuid]",
        ".meta.started_at" => "[timestamp]",
        ".meta.finished_at" => "[timestamp]",
        ".meta.duration_ms" => "[duration]",
        ".trace[].ts" => "[timestamp]",
        ".receipt_sha256" => "[hash]",
        ".verification.git_diff" => "[git_diff]",
        ".verification.git_status" => "[git_status]",
    });
}

// ---------------------------------------------------------------------------
// 2. Events snapshot from a mock backend run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_mock_events() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    assert_json_snapshot!("pipeline_mock_events", events, {
        "[].ts" => "[timestamp]",
    });
}

// ---------------------------------------------------------------------------
// 3. Receipt with full metadata (backend, capabilities, trace)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_receipt_full_metadata() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;

    // Snapshot backend identity
    assert_json_snapshot!("pipeline_backend_identity", receipt.backend);

    // Snapshot capabilities manifest
    let caps_value = serde_json::to_value(&receipt.capabilities).unwrap();
    assert_json_snapshot!("pipeline_capabilities", caps_value);

    // Snapshot trace event kinds (redact timestamps)
    let trace_value = serde_json::to_value(&receipt.trace).unwrap();
    assert_json_snapshot!("pipeline_trace", trace_value, {
        "[].ts" => "[timestamp]",
    });
}

// ---------------------------------------------------------------------------
// 4. RuntimeError messages for each variant
// ---------------------------------------------------------------------------

#[test]
fn snapshot_runtime_error_unknown_backend() {
    let err = RuntimeError::UnknownBackend {
        name: "nonexistent".into(),
    };
    assert_snapshot!("pipeline_error_unknown_backend", err.to_string());
}

#[test]
fn snapshot_runtime_error_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_snapshot!("pipeline_error_workspace_failed", err.to_string());
}

#[test]
fn snapshot_runtime_error_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("invalid glob pattern"));
    assert_snapshot!("pipeline_error_policy_failed", err.to_string());
}

#[test]
fn snapshot_runtime_error_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("connection refused"));
    assert_snapshot!("pipeline_error_backend_failed", err.to_string());
}

#[test]
fn snapshot_runtime_error_capability_check_failed() {
    let err = RuntimeError::CapabilityCheckFailed(
        "missing capability: mcp_client requires native but got unsupported".into(),
    );
    assert_snapshot!("pipeline_error_capability_check_failed", err.to_string());
}

// ---------------------------------------------------------------------------
// 5. Metrics snapshot after runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_metrics_after_runs() {
    let rt = Runtime::with_default_backends();

    // Run two work orders to accumulate metrics.
    run_to_completion(&rt, mock_work_order()).await;
    run_to_completion(&rt, mock_work_order()).await;

    let snap = rt.metrics().snapshot();

    assert_json_snapshot!("pipeline_metrics", snap, {
        ".average_run_duration_ms" => "[duration]",
    });
}
