#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end integration tests exercising the full ABP pipeline:
//! CLI-level work order → Runtime → Backend → Receipt.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    receipt_hash, AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RunMetadata, RuntimeConfig, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_runtime::store::ReceiptStore;
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a [`RunHandle`].
async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("backend task panicked");
    (collected, receipt)
}

/// A backend that always returns an error, for negative-path testing.
#[derive(Debug, Clone)]
struct FailingBackend;

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("intentional failure for testing")
    }
}

/// A backend that echoes events and captures the work order for inspection.
#[derive(Debug, Clone)]
struct EchoBackend {
    label: String,
}

#[async_trait]
impl Backend for EchoBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.label.clone(),
            backend_version: Some("0.1.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, abp_core::SupportLevel::Native);
        m.insert(Capability::ToolRead, abp_core::SupportLevel::Emulated);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();
        let mut trace = Vec::new();

        let ev_start = AgentEvent {
            ts: started_at,
            kind: AgentEventKind::RunStarted {
                message: format!("echo-{} started", self.label),
            },
            ext: None,
        };
        trace.push(ev_start.clone());
        let _ = events_tx.send(ev_start).await;

        let ev_msg = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("echo: {}", work_order.task),
            },
            ext: None,
        };
        trace.push(ev_msg.clone());
        let _ = events_tx.send(ev_msg).await;

        let ev_done = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("echo-{} done", self.label),
            },
            ext: None,
        };
        trace.push(ev_done.clone());
        let _ = events_tx.send(ev_done).await;

        let finished_at = chrono::Utc::now();
        let duration_ms = (finished_at - started_at)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        let mode = abp_integrations::extract_execution_mode(&work_order);

        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode,
            usage_raw: serde_json::json!({"note": "echo"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?;

        Ok(receipt)
    }
}

// ===========================================================================
// 1. Full pipeline: work order → backend selection → execution → receipt → hash
// ===========================================================================

#[tokio::test]
async fn full_pipeline_work_order_to_hashed_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("full pipeline e2e task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Receipt identity matches.
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.run_id, run_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);

    // Outcome is complete.
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Hash present, 64-char hex, deterministic.
    let hash = receipt.receipt_sha256.as_ref().expect("hash present");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());

    // Events were streamed.
    assert!(!events.is_empty());
    // Trace on receipt matches events.
    assert_eq!(receipt.trace.len(), events.len());

    // Backend identity is mock.
    assert_eq!(receipt.backend.id, "mock");

    // Timing sanity.
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

// ===========================================================================
// 2. MockBackend E2E: receipt comes back with correct hash
// ===========================================================================

#[tokio::test]
async fn mock_backend_receipt_hash_is_verifiable() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("hash verification test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Recompute hash and verify it matches the stored one.
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(
        receipt.receipt_sha256.as_deref(),
        Some(recomputed.as_str()),
        "stored hash must match recomputed hash"
    );

    // Hash is stable across multiple computations.
    let recomputed2 = receipt_hash(&receipt).unwrap();
    assert_eq!(recomputed, recomputed2, "hash must be deterministic");
}

// ===========================================================================
// 3. Multi-backend E2E: switch between backends with different configs
// ===========================================================================

#[tokio::test]
async fn multi_backend_switching() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "echo-alpha",
        EchoBackend {
            label: "echo-alpha".into(),
        },
    );
    rt.register_backend(
        "echo-beta",
        EchoBackend {
            label: "echo-beta".into(),
        },
    );
    rt.register_backend("mock", abp_integrations::MockBackend);

    // Run on alpha.
    let wo = WorkOrderBuilder::new("alpha task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("echo-alpha", wo).await.unwrap();
    let (events_a, receipt_a) = drain_run(handle).await;
    let receipt_a = receipt_a.unwrap();
    assert_eq!(receipt_a.backend.id, "echo-alpha");
    assert_eq!(receipt_a.outcome, Outcome::Complete);
    assert!(!events_a.is_empty());

    // Run on beta.
    let wo = WorkOrderBuilder::new("beta task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("echo-beta", wo).await.unwrap();
    let (_events_b, receipt_b) = drain_run(handle).await;
    let receipt_b = receipt_b.unwrap();
    assert_eq!(receipt_b.backend.id, "echo-beta");
    assert_eq!(receipt_b.outcome, Outcome::Complete);

    // Run on mock.
    let wo = WorkOrderBuilder::new("mock task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_m) = drain_run(handle).await;
    let receipt_m = receipt_m.unwrap();
    assert_eq!(receipt_m.backend.id, "mock");

    // All run IDs are unique.
    let ids: std::collections::HashSet<_> = [
        receipt_a.meta.run_id,
        receipt_b.meta.run_id,
        receipt_m.meta.run_id,
    ]
    .into_iter()
    .collect();
    assert_eq!(ids.len(), 3, "run IDs must be unique across backends");

    // All hashes valid.
    for r in [&receipt_a, &receipt_b, &receipt_m] {
        let h = receipt_hash(r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref(), Some(h.as_str()));
    }
}

// ===========================================================================
// 4. Error propagation E2E: errors at each layer propagate correctly
// ===========================================================================

#[tokio::test]
async fn error_propagation_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("no such backend")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("nonexistent_backend", wo).await {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent_backend");
        }
        _ => panic!("expected UnknownBackend, got unexpected result"),
    }
}

#[tokio::test]
async fn error_propagation_backend_failure() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("trigger backend failure")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let err = receipt.unwrap_err();
    assert!(
        matches!(err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed, got {err:?}"
    );
}

#[tokio::test]
async fn error_propagation_capability_mismatch() {
    let rt = Runtime::with_default_backends();

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    // Pre-flight check surfaces error.
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));

    // Running also fails.
    let wo = WorkOrderBuilder::new("cap mismatch")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_code_classification() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(!err.is_retryable());

    let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
}

// ===========================================================================
// 5. Event streaming E2E: events flow from backend through runtime to caller
// ===========================================================================

#[tokio::test]
async fn event_streaming_order_and_completeness() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("event stream test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // MockBackend emits at least RunStarted + messages + RunCompleted.
    assert!(
        events.len() >= 3,
        "expected ≥3 events, got {}",
        events.len()
    );

    // First event is RunStarted.
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted"
    );

    // Last event is RunCompleted.
    assert!(
        matches!(
            &events[events.len() - 1].kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );

    // Timestamps are monotonically non-decreasing.
    for pair in events.windows(2) {
        assert!(
            pair[1].ts >= pair[0].ts,
            "events must be chronologically ordered"
        );
    }

    // Trace matches streamed events.
    assert_eq!(
        receipt.trace.len(),
        events.len(),
        "receipt trace must match streamed event count"
    );
}

#[tokio::test]
async fn event_streaming_custom_backend_content() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "echo",
        EchoBackend {
            label: "echo".into(),
        },
    );

    let wo = WorkOrderBuilder::new("my custom task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("echo", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Echo backend emits exactly 3 events.
    assert_eq!(events.len(), 3);

    // The middle event contains the echoed task text.
    if let AgentEventKind::AssistantMessage { ref text } = events[1].kind {
        assert!(
            text.contains("my custom task"),
            "echo backend should echo the task"
        );
    } else {
        panic!("expected AssistantMessage in middle event");
    }

    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 6. Config integration E2E: passthrough vs mapped, vendor flags
// ===========================================================================

#[tokio::test]
async fn config_passthrough_mode() {
    let rt = Runtime::with_default_backends();

    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );

    let wo = WorkOrderBuilder::new("passthrough config")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn config_mapped_mode_default() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("mapped mode default")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn config_model_and_budget_propagated() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("config propagation")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4o")
        .max_budget_usd(5.0)
        .max_turns(10)
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(10));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn config_with_policy_and_passthrough() {
    let rt = Runtime::with_default_backends();

    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };

    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );

    let wo = WorkOrderBuilder::new("policy + passthrough")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 7. Receipt store E2E: submit work, store receipt, query it back
// ===========================================================================

#[tokio::test]
async fn receipt_store_save_load_verify() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("receipt store test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let run_id = receipt.meta.run_id;

    // Save.
    let path = store.save(&receipt).unwrap();
    assert!(path.exists());

    // Load.
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.outcome, Outcome::Complete);
    assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(loaded.backend.id, "mock");

    // Verify hash integrity.
    assert!(store.verify(run_id).unwrap(), "hash verification must pass");

    // List.
    let ids = store.list().unwrap();
    assert!(ids.contains(&run_id));
}

#[tokio::test]
async fn receipt_store_chain_verification() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let mut run_ids = Vec::new();
    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("chain task {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        run_ids.push(receipt.meta.run_id);
        store.save(&receipt).unwrap();
    }

    // All receipts loadable and verifiable.
    for id in &run_ids {
        assert!(store.verify(*id).unwrap());
    }

    // Chain verification.
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid, "receipt chain should be valid");
    assert_eq!(chain.valid_count, 3);
    assert!(chain.invalid_hashes.is_empty());
    assert_eq!(chain.gaps.len(), 2); // 3 runs → 2 gaps
}

#[tokio::test]
async fn receipt_store_empty_chain_is_valid() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 0);
}

// ===========================================================================
// 8. Workspace E2E: staging, execution, cleanup lifecycle
// ===========================================================================

#[tokio::test]
async fn workspace_staging_lifecycle() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(src_dir.path().join("src").join("lib.rs"), "pub fn lib() {}").unwrap();
    std::fs::write(src_dir.path().join("secret.key"), "private").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("workspace lifecycle test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .exclude(vec!["*.key".into()])
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    // Staged workspace produces git-based verification.
    assert!(
        receipt.verification.git_diff.is_some() || receipt.verification.git_status.is_some(),
        "staged workspace should produce git verification data"
    );
}

#[tokio::test]
async fn workspace_passthrough_no_staging() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
}

// ===========================================================================
// 9. Concurrent pipeline execution
// ===========================================================================

#[tokio::test]
async fn concurrent_pipeline_execution() {
    let rt = Runtime::with_default_backends();

    let mut handles = Vec::new();
    for i in 0..5 {
        let wo = WorkOrderBuilder::new(format!("concurrent-{i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        handles.push(rt.run_streaming("mock", wo).await.unwrap());
    }

    let mut receipts = Vec::new();
    for handle in handles {
        let (_, receipt) = drain_run(handle).await;
        receipts.push(receipt.unwrap());
    }

    assert_eq!(receipts.len(), 5);

    // All run IDs unique.
    let ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 5);

    // All completed with valid hashes.
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
        let h = receipt_hash(r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref(), Some(h.as_str()));
    }
}

// ===========================================================================
// 10. Telemetry metrics updated after pipeline runs
// ===========================================================================

#[tokio::test]
async fn telemetry_metrics_after_pipeline_runs() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.metrics().snapshot().total_runs, 0);

    // Two successful runs.
    for _ in 0..2 {
        let wo = WorkOrderBuilder::new("metrics run")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        receipt.unwrap();
    }

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0);
}

// ===========================================================================
// 11. Backend registry integration
// ===========================================================================

#[tokio::test]
async fn backend_registry_list_and_lookup() {
    let mut rt = Runtime::new();
    assert!(rt.backend_names().is_empty());

    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend(
        "echo",
        EchoBackend {
            label: "echo".into(),
        },
    );

    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
    assert!(names.contains(&"echo".to_string()));

    assert!(rt.backend("mock").is_some());
    assert!(rt.backend("echo").is_some());
    assert!(rt.backend("nonexistent").is_none());
}

// ===========================================================================
// 12. Policy engine integration
// ===========================================================================

#[tokio::test]
async fn policy_engine_enforces_restrictions() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_read: vec!["**/.env".into(), "**/.secret".into()],
        deny_write: vec!["**/production/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Tool restrictions.
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);

    // Path restrictions.
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("config/.secret")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("production/db.sql"))
            .allowed
    );
    assert!(engine.can_write_path(Path::new("staging/db.sql")).allowed);

    // Pipeline still completes with restrictive policy.
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy enforcement test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 13. Large and minimal work order edge cases
// ===========================================================================

#[tokio::test]
async fn large_work_order_pipeline() {
    let large_task = "x".repeat(50 * 1024); // 50 KB task
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new(large_task.clone())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert_eq!(wo.task.len(), 50 * 1024);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    let h = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(h.as_str()));
}

#[tokio::test]
async fn minimal_empty_task_pipeline() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.receipt_sha256.is_some());
    assert!(!events.is_empty());
}

// ===========================================================================
// 14. Receipt chain via runtime
// ===========================================================================

#[tokio::test]
async fn runtime_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("chain run {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        receipt.unwrap();
    }

    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 3, "receipt chain should accumulate 3 receipts");
}

// ===========================================================================
// 15. Execution mode toggle between runs
// ===========================================================================

#[tokio::test]
async fn execution_mode_toggle_between_runs() {
    let rt = Runtime::with_default_backends();

    // Default mapped.
    let wo = WorkOrderBuilder::new("mapped run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, r) = drain_run(handle).await;
    assert_eq!(r.unwrap().mode, ExecutionMode::Mapped);

    // Passthrough.
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo = WorkOrderBuilder::new("passthrough run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, r) = drain_run(handle).await;
    assert_eq!(r.unwrap().mode, ExecutionMode::Passthrough);

    // Back to mapped.
    let wo = WorkOrderBuilder::new("mapped again")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, r) = drain_run(handle).await;
    assert_eq!(r.unwrap().mode, ExecutionMode::Mapped);
}

// ===========================================================================
// 16. RuntimeError properties
// ===========================================================================

#[tokio::test]
async fn runtime_error_retryable_classification() {
    // Non-retryable errors.
    let non_retryable = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob")),
        RuntimeError::CapabilityCheckFailed("missing cap".into()),
        RuntimeError::NoProjectionMatch {
            reason: "no match".into(),
        },
    ];
    for err in &non_retryable {
        assert!(!err.is_retryable(), "{err:?} should not be retryable");
    }

    // Retryable errors.
    let retryable = vec![
        RuntimeError::BackendFailed(anyhow::anyhow!("timeout")),
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full")),
    ];
    for err in &retryable {
        assert!(err.is_retryable(), "{err:?} should be retryable");
    }
}

// ===========================================================================
// 17. Receipt hash self-referential prevention
// ===========================================================================

#[tokio::test]
async fn receipt_hash_excludes_itself() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("hash self-ref test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Modifying the hash field should not affect the recomputed hash.
    let mut tampered = receipt.clone();
    tampered.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    let hash_from_tampered = receipt_hash(&tampered).unwrap();
    let hash_from_original = receipt_hash(&receipt).unwrap();
    assert_eq!(
        hash_from_tampered, hash_from_original,
        "receipt_hash must ignore receipt_sha256 field"
    );
}

// ===========================================================================
// 18. Store + pipeline round-trip
// ===========================================================================

#[tokio::test]
async fn store_pipeline_roundtrip() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    // Run multiple tasks and store receipts.
    let mut receipts = Vec::new();
    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("roundtrip-{i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        store.save(&receipt).unwrap();
        receipts.push(receipt);
    }

    // Verify all persisted receipts match originals.
    for original in &receipts {
        let loaded = store.load(original.meta.run_id).unwrap();
        assert_eq!(loaded.meta.run_id, original.meta.run_id);
        assert_eq!(loaded.receipt_sha256, original.receipt_sha256);
        assert_eq!(loaded.outcome, original.outcome);
        assert_eq!(loaded.backend.id, original.backend.id);
        assert!(store.verify(original.meta.run_id).unwrap());
    }
}
