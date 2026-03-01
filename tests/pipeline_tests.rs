// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-crate pipeline integration tests exercising the full ABP stack.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    receipt_hash,
};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_runtime::store::ReceiptStore;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
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

// ===========================================================================
// 1. Happy path
// ===========================================================================

#[tokio::test]
async fn happy_path_mock_backend() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("hello world")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);

    // Hash is present, 64-char hex, and deterministic.
    let hash = receipt.receipt_sha256.as_ref().expect("hash should be set");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());

    // Events were streamed and recorded.
    assert!(!events.is_empty());
    assert!(!receipt.trace.is_empty());
}

// ===========================================================================
// 2. Policy enforcement
// ===========================================================================

#[tokio::test]
async fn policy_enforcement_blocks_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Disallowed tools are blocked.
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);

    // Path restrictions enforced.
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_write_path(Path::new("secret/data.txt")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);

    // Full pipeline still completes with the restrictive policy attached.
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("task with policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 3. Workspace staging
// ===========================================================================

#[tokio::test]
async fn workspace_staging_pipeline() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(src_dir.path().join("secret.key"), "private").unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(src_dir.path().join("src").join("lib.rs"), "pub fn lib() {}").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("workspace test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .exclude(vec!["*.key".into()])
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    // Verification report is populated from the staged git workspace.
    assert!(receipt.verification.git_diff.is_some() || receipt.verification.git_status.is_some());
}

// ===========================================================================
// 4. Event streaming order and types
// ===========================================================================

#[tokio::test]
async fn event_streaming_order_and_types() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("event streaming test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // MockBackend emits: RunStarted, 2× AssistantMessage, RunCompleted.
    assert!(
        events.len() >= 4,
        "expected at least 4 events, got {}",
        events.len()
    );

    // First event should be RunStarted.
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got {:?}",
        events[0].kind
    );

    // Last event should be RunCompleted.
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
            "events should be chronologically ordered"
        );
    }

    // Trace in receipt matches streamed events.
    assert_eq!(receipt.trace.len(), events.len());
}

// ===========================================================================
// 5. Config propagation
// ===========================================================================

#[tokio::test]
async fn config_propagation_vendor_flags() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough", "custom_key": "custom_value"}),
    );

    let config = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        max_budget_usd: Some(1.0),
        max_turns: Some(5),
        ..Default::default()
    };

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("config test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
    assert_eq!(wo.config.max_turns, Some(5));
    assert!(wo.config.vendor.contains_key("abp"));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Vendor config set passthrough mode.
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 6. Receipt chain integrity
// ===========================================================================

#[tokio::test]
async fn receipt_chain_integrity() {
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

    // Every receipt is loadable and verifiable.
    for id in &run_ids {
        let loaded = store.load(*id).unwrap();
        assert!(store.verify(*id).unwrap(), "hash should verify for {id}");
        assert_eq!(loaded.outcome, Outcome::Complete);
    }

    // Chain verification passes.
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid, "chain should be valid");
    assert_eq!(chain.valid_count, 3);
    assert!(chain.invalid_hashes.is_empty());
    assert_eq!(chain.gaps.len(), 2); // 3 runs → 2 gaps
}

// ===========================================================================
// 7. Concurrent pipelines
// ===========================================================================

#[tokio::test]
async fn concurrent_pipelines() {
    let rt = Runtime::with_default_backends();

    // Launch three runs concurrently (each spawns its own task).
    let mut handles = Vec::new();
    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("concurrent task {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        handles.push(rt.run_streaming("mock", wo).await.unwrap());
    }

    let mut receipts = Vec::new();
    for handle in handles {
        let (_, receipt) = drain_run(handle).await;
        receipts.push(receipt.unwrap());
    }

    assert_eq!(receipts.len(), 3);

    // All run IDs are unique.
    let ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 3, "run IDs should be unique");

    // All completed with valid hashes.
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
        let hash = receipt_hash(r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref(), Some(hash.as_str()));
    }
}

// ===========================================================================
// 8. Error propagation
// ===========================================================================

#[tokio::test]
async fn error_propagation_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("should fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("nonexistent", wo).await {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent");
        }
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_propagation_failing_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("should fail")
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

// ===========================================================================
// 9. Capability checks
// ===========================================================================

#[tokio::test]
async fn capability_check_satisfied() {
    let rt = Runtime::with_default_backends();

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    rt.check_capabilities("mock", &reqs).unwrap();

    // Pipeline should also complete with these requirements.
    let wo = WorkOrderBuilder::new("cap check test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn capability_check_unsatisfied() {
    let rt = Runtime::with_default_backends();

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    // Pre-flight check fails.
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));

    // Pipeline also rejects.
    let wo = WorkOrderBuilder::new("will fail cap check")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ===========================================================================
// 10. Execution modes
// ===========================================================================

#[tokio::test]
async fn execution_mode_passthrough_vs_mapped() {
    let rt = Runtime::with_default_backends();

    // Default (mapped) mode.
    let wo_mapped = WorkOrderBuilder::new("mapped mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo_mapped).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt_mapped = receipt.unwrap();
    assert_eq!(receipt_mapped.mode, ExecutionMode::Mapped);

    // Passthrough mode via vendor config.
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo_pt = WorkOrderBuilder::new("passthrough mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let handle = rt.run_streaming("mock", wo_pt).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt_pt = receipt.unwrap();
    assert_eq!(receipt_pt.mode, ExecutionMode::Passthrough);

    // Both completed successfully.
    assert_eq!(receipt_mapped.outcome, Outcome::Complete);
    assert_eq!(receipt_pt.outcome, Outcome::Complete);
}

// ===========================================================================
// 11. Large work order
// ===========================================================================

#[tokio::test]
async fn large_work_order_pipeline() {
    let large_task = "x".repeat(10 * 1024); // 10 KB
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new(large_task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert_eq!(wo.task.len(), 10240);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());
}

// ===========================================================================
// 12. Minimal work order
// ===========================================================================

#[tokio::test]
async fn minimal_work_order_pipeline() {
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
    assert!(!receipt.trace.is_empty());
    assert!(!events.is_empty());
    assert_eq!(receipt.backend.id, "mock");
}

// ===========================================================================
// 13. Telemetry metrics updated after runs
// ===========================================================================

#[tokio::test]
async fn telemetry_metrics_updated() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.metrics().snapshot().total_runs, 0);

    for _ in 0..2 {
        let wo = WorkOrderBuilder::new("metrics test")
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
// 14. Backend registry integration
// ===========================================================================

#[tokio::test]
async fn backend_registry_integration() {
    let mut rt = Runtime::new();
    assert!(rt.backend_names().is_empty());

    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend("mock2", abp_integrations::MockBackend);

    assert_eq!(rt.backend_names(), vec!["mock", "mock2"]);
    assert!(rt.backend("mock").is_some());
    assert!(rt.backend("nonexistent").is_none());

    let wo = WorkOrderBuilder::new("registry test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock2", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}
