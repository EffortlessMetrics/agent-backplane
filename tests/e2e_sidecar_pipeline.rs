#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! End-to-end integration tests for the sidecar backend pipeline.
//!
//! These tests exercise the full ABP pipeline (WorkOrder → Backend → Events → Receipt)
//! using MockBackend and custom test backends — no actual sidecar processes are needed.
//!
//! Sections:
//! 1. MockBackend pipeline (15 tests)
//! 2. Runtime integration (15 tests)
//! 3. Event stream processing (10 tests)
//! 4. Error paths (10+ tests)

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, WorkOrder, WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

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

/// Build and execute a simple mock run, returning events and receipt.
async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

/// A backend that always returns an error.
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

/// A backend that emits a configurable number of assistant messages.
#[derive(Debug, Clone)]
struct ConfigurableBackend {
    message_count: usize,
    identity_name: String,
}

#[async_trait]
impl Backend for ConfigurableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.identity_name.clone(),
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();
        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.identity_name),
            },
            ext: None,
        };
        let _ = events_tx.send(start_ev.clone()).await;
        let mut trace = vec![start_ev];

        for i in 0..self.message_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("message {i} from {}", self.identity_name),
                },
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(end_ev.clone()).await;
        trace.push(end_ev);

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
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
}

/// A backend that emits interleaved event types for ordering tests.
#[derive(Debug, Clone)]
struct OrderingBackend;

#[async_trait]
impl Backend for OrderingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "ordering".into(),
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();
        let mut trace = Vec::new();

        let kinds = [
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "thinking".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "test.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!("file content"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "updated".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ];

        for kind in kinds {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
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
}

/// A slow backend that sleeps before completing.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay_ms: u64,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".into(),
            backend_version: None,
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "slow start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        let mut trace = vec![ev];

        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "slow done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
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
}

/// A backend that emits events but returns a partial outcome.
#[derive(Debug, Clone)]
struct PartialBackend;

#[async_trait]
impl Backend for PartialBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial".into(),
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "partial start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        let mut trace = vec![ev];

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Warning {
                message: "budget exhausted".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "partial done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Partial,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// A backend that emits error events and returns Failed outcome.
#[derive(Debug, Clone)]
struct ErrorEventBackend;

#[async_trait]
impl Backend for ErrorEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error_event".into(),
            backend_version: None,
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "error start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        let mut trace = vec![ev];

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: None,
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "error done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Failed,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// A backend that reports specific capabilities for requirement tests.
#[derive(Debug, Clone)]
struct CapableBackend {
    caps: CapabilityManifest,
}

#[async_trait]
impl Backend for CapableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "capable".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        self.caps.clone()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        abp_backend_core::ensure_capability_requirements(
            &work_order.requirements,
            &self.capabilities(),
        )?;
        let started_at = chrono::Utc::now();
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "capable start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        let trace = vec![ev];

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
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
}

// ===========================================================================
// 1. MockBackend pipeline (15 tests)
// ===========================================================================

#[tokio::test]
async fn mock_pipeline_returns_complete_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "basic run").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_pipeline_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "hash check").await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn mock_pipeline_hash_is_deterministic_for_same_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "hash determinism").await;
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[tokio::test]
async fn mock_pipeline_events_arrive_in_order() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "ordering test").await;

    assert!(
        events.len() >= 4,
        "expected ≥4 events, got {}",
        events.len()
    );
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn mock_pipeline_middle_events_are_assistant_messages() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "middle events").await;

    for ev in &events[1..events.len() - 1] {
        assert!(
            matches!(&ev.kind, AgentEventKind::AssistantMessage { .. }),
            "expected AssistantMessage, got {:?}",
            ev.kind
        );
    }
}

#[tokio::test]
async fn mock_pipeline_contract_version_in_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "contract version").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_pipeline_backend_identity_correct() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "identity check").await;
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn mock_pipeline_work_order_id_preserved() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("id test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let wo_id = wo.id;

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn mock_pipeline_trace_in_receipt_non_empty() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "trace check").await;
    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should contain events"
    );
}

#[tokio::test]
async fn mock_pipeline_receipt_timestamps_ordered() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "timestamp order").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn mock_pipeline_usage_normalized_present() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "usage check").await;
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn mock_pipeline_execution_mode_default_mapped() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "mode check").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mock_pipeline_capabilities_populated() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "caps check").await;
    assert!(
        receipt.capabilities.contains_key(&Capability::Streaming),
        "mock backend should report Streaming capability"
    );
}

#[tokio::test]
async fn mock_pipeline_event_count_matches_trace() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "count match").await;
    // The runtime-observed events should match the trace the backend returned.
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn mock_pipeline_receipt_serializes_roundtrip() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "serde roundtrip").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.outcome, receipt.outcome);
    assert_eq!(deserialized.meta.work_order_id, receipt.meta.work_order_id);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
}

// ===========================================================================
// 2. Runtime integration (15 tests)
// ===========================================================================

#[tokio::test]
async fn runtime_register_and_list_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend(
        "custom",
        ConfigurableBackend {
            message_count: 1,
            identity_name: "custom".into(),
        },
    );

    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
    assert!(names.contains(&"custom".to_string()));
}

#[tokio::test]
async fn runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn runtime_routing_to_correct_backend() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "alpha",
        ConfigurableBackend {
            message_count: 2,
            identity_name: "alpha".into(),
        },
    );
    rt.register_backend(
        "beta",
        ConfigurableBackend {
            message_count: 5,
            identity_name: "beta".into(),
        },
    );

    let wo = WorkOrderBuilder::new("route alpha")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("alpha", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.backend.id, "alpha");
    // RunStarted + 2 messages + RunCompleted = 4
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn runtime_sequential_runs_produce_distinct_receipts() {
    let rt = Runtime::with_default_backends();
    let (_events1, receipt1) = run_mock(&rt, "run 1").await;
    let (_events2, receipt2) = run_mock(&rt, "run 2").await;

    assert_ne!(receipt1.meta.run_id, receipt2.meta.run_id);
    assert_ne!(receipt1.meta.work_order_id, receipt2.meta.work_order_id);
    // Hashes differ because content differs
    assert_ne!(receipt1.receipt_sha256, receipt2.receipt_sha256);
}

#[tokio::test]
async fn runtime_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain 1").await;
    run_mock(&rt, "chain 2").await;

    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert!(locked.len() >= 2, "chain should have ≥2 receipts");
}

#[tokio::test]
async fn runtime_workspace_passthrough_succeeds() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_workspace_staged_succeeds() {
    let rt = Runtime::with_default_backends();
    let tmp = tempfile::tempdir().unwrap();
    // Write a small file so the staged workspace copy is minimal.
    std::fs::write(tmp.path().join("hello.txt"), "world").unwrap();
    let wo = WorkOrderBuilder::new("staged test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(tmp.path().to_string_lossy().to_string())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_policy_empty_permits_everything() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy empty")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile::default())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_policy_with_tool_restrictions() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("policy restricted")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    // The policy compiles and the run proceeds (enforcement is best-effort in v0.1)
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_policy_with_deny_paths() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/secrets/**".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("deny paths")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_metrics_recorded_after_run() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "metrics test").await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_runs >= 1, "should record at least one run");
}

#[tokio::test]
async fn runtime_configurable_backend_message_count() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "ten_msgs",
        ConfigurableBackend {
            message_count: 10,
            identity_name: "ten_msgs".into(),
        },
    );

    let wo = WorkOrderBuilder::new("many messages")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ten_msgs", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    // RunStarted + 10 messages + RunCompleted = 12
    assert_eq!(events.len(), 12);
}

#[tokio::test]
async fn runtime_backend_replacement() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "slot",
        ConfigurableBackend {
            message_count: 1,
            identity_name: "first".into(),
        },
    );
    rt.register_backend(
        "slot",
        ConfigurableBackend {
            message_count: 3,
            identity_name: "second".into(),
        },
    );

    let wo = WorkOrderBuilder::new("replacement")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("slot", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.backend.id, "second");
    // RunStarted + 3 messages + RunCompleted = 5
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn runtime_check_capabilities_passes() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[tokio::test]
async fn runtime_check_capabilities_fails_unsatisfied() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

// ===========================================================================
// 3. Event stream processing (10 tests)
// ===========================================================================

#[tokio::test]
async fn events_all_have_timestamps() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "timestamp presence").await;
    for ev in &events {
        // All events should have a valid UTC timestamp
        assert!(ev.ts.timestamp() > 0, "event timestamp should be positive");
    }
}

#[tokio::test]
async fn events_timestamps_are_non_decreasing() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "timestamp ordering").await;
    for window in events.windows(2) {
        assert!(
            window[0].ts <= window[1].ts,
            "event timestamps should be non-decreasing: {:?} > {:?}",
            window[0].ts,
            window[1].ts
        );
    }
}

#[tokio::test]
async fn events_filter_by_assistant_message() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "filter assistant").await;
    let assistant_msgs: Vec<_> = events
        .iter()
        .filter(|ev| matches!(&ev.kind, AgentEventKind::AssistantMessage { .. }))
        .collect();
    assert!(
        assistant_msgs.len() >= 2,
        "mock emits ≥2 assistant messages"
    );
}

#[tokio::test]
async fn events_filter_by_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "filter started").await;
    let started: Vec<_> = events
        .iter()
        .filter(|ev| matches!(&ev.kind, AgentEventKind::RunStarted { .. }))
        .collect();
    assert_eq!(started.len(), 1, "exactly one RunStarted event expected");
}

#[tokio::test]
async fn events_filter_by_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "filter completed").await;
    let completed: Vec<_> = events
        .iter()
        .filter(|ev| matches!(&ev.kind, AgentEventKind::RunCompleted { .. }))
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "exactly one RunCompleted event expected"
    );
}

#[tokio::test]
async fn events_ordering_backend_preserves_sequence() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("ordering test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    // Expected order: RunStarted, AssistantMessage, ToolCall, ToolResult, FileChanged, RunCompleted
    assert_eq!(events.len(), 6);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[2].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(&events[3].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(
        &events[4].kind,
        AgentEventKind::FileChanged { .. }
    ));
    assert!(matches!(
        &events[5].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn events_tool_call_has_correct_fields() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("tool call fields")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let tool_call = events
        .iter()
        .find(|ev| matches!(&ev.kind, AgentEventKind::ToolCall { .. }))
        .expect("should have a ToolCall event");

    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        ..
    } = &tool_call.kind
    {
        assert_eq!(tool_name, "Read");
        assert_eq!(tool_use_id.as_deref(), Some("tc1"));
    }
}

#[tokio::test]
async fn events_tool_result_matches_tool_call_id() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("tool result match")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let call_id = events.iter().find_map(|ev| {
        if let AgentEventKind::ToolCall { tool_use_id, .. } = &ev.kind {
            tool_use_id.clone()
        } else {
            None
        }
    });
    let result_id = events.iter().find_map(|ev| {
        if let AgentEventKind::ToolResult { tool_use_id, .. } = &ev.kind {
            tool_use_id.clone()
        } else {
            None
        }
    });

    assert_eq!(
        call_id, result_id,
        "tool_use_id should match between call and result"
    );
}

#[tokio::test]
async fn events_ext_field_is_none_for_mock() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "ext field check").await;
    for ev in &events {
        assert!(ev.ext.is_none(), "mock events should not have ext data");
    }
}

#[tokio::test]
async fn events_partial_backend_includes_warning() {
    let mut rt = Runtime::new();
    rt.register_backend("partial", PartialBackend);

    let wo = WorkOrderBuilder::new("partial events")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("partial", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Partial);
    let warnings: Vec<_> = events
        .iter()
        .filter(|ev| matches!(&ev.kind, AgentEventKind::Warning { .. }))
        .collect();
    assert_eq!(warnings.len(), 1);
}

// ===========================================================================
// 4. Error paths (10+ tests)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("unknown")
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
async fn error_unknown_backend_has_error_code() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("error code")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("no_such_backend", wo).await {
        Err(e) => assert_eq!(e.error_code(), abp_error::ErrorCode::BackendNotFound),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_failing_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("failure test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;

    assert!(receipt.is_err(), "failing backend should produce an error");
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn error_failing_backend_error_message() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("failure msg")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;

    let err = receipt.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("backend execution failed"),
        "error message should mention backend failure: {msg}"
    );
}

#[tokio::test]
async fn error_capability_check_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("nope", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn error_unsatisfied_capability_native() {
    let mut rt = Runtime::new();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, abp_core::SupportLevel::Native);
    rt.register_backend("capable", CapableBackend { caps });

    let wo = WorkOrderBuilder::new("cap fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let handle = rt.run_streaming("capable", wo).await;
    // Should fail at capability check
    assert!(handle.is_err(), "should fail capability check");
}

#[tokio::test]
async fn error_emulated_does_not_satisfy_native() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    // MockBackend has ToolRead as Emulated, which doesn't satisfy Native
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[tokio::test]
async fn error_error_event_backend_returns_failed_outcome() {
    let mut rt = Runtime::new();
    rt.register_backend("error_event", ErrorEventBackend);

    let wo = WorkOrderBuilder::new("error events")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("error_event", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Failed);
    let errors: Vec<_> = events
        .iter()
        .filter(|ev| matches!(&ev.kind, AgentEventKind::Error { .. }))
        .collect();
    assert_eq!(errors.len(), 1);
}

#[tokio::test]
async fn error_empty_runtime_rejects_all_backends() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("empty runtime")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let result = rt.run_streaming("anything", wo).await;
    match result {
        Err(RuntimeError::UnknownBackend { .. }) => {}
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_runtime_error_to_abp_error_conversion() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("test"));
}

#[tokio::test]
async fn error_slow_backend_completes_eventually() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 50 });

    let wo = WorkOrderBuilder::new("slow test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("slow", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(events.len() >= 2);
    assert!(
        receipt.meta.duration_ms >= 40,
        "slow backend should take ≥40ms, took {}ms",
        receipt.meta.duration_ms
    );
}

#[tokio::test]
async fn error_multiple_unknown_backends_distinct_names() {
    let rt = Runtime::new();
    for name in ["x", "y", "z"] {
        let wo = WorkOrderBuilder::new("test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        match rt.run_streaming(name, wo).await {
            Err(RuntimeError::UnknownBackend { name: n }) => assert_eq!(n, name),
            Err(e) => panic!("expected UnknownBackend for {name}, got {e:?}"),
            Ok(_) => panic!("expected error for {name}, got Ok"),
        }
    }
}
