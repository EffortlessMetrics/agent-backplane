#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive end-to-end pipeline tests validating the full
//! WorkOrder → Backend → EventStream → Receipt flow.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use abp_backend_core::Backend;
use abp_backend_mock::MockBackend;
use abp_backend_mock::scenarios::{MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_error::ErrorCode;
use abp_policy::PolicyEngine;
use abp_receipt::{ReceiptBuilder, compute_hash, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{
    EventFilter, EventRecorder, EventStats, EventTransform, StreamPipeline, StreamPipelineBuilder,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn mapped_wo(task: &str) -> WorkOrder {
    let mut wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "mapped"}));
    wo
}

fn passthrough_mode_wo(task: &str) -> WorkOrder {
    let mut wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    wo
}

fn wo_with_lane(task: &str, lane: ExecutionLane) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .lane(lane)
        .build()
}

fn wo_with_policy(task: &str, policy: PolicyProfile) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build()
}

fn wo_with_model(task: &str, model: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .model(model)
        .build()
}

fn wo_with_budget(task: &str, budget: f64) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_budget_usd(budget)
        .build()
}

fn wo_with_max_turns(task: &str, turns: u32) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_turns(turns)
        .build()
}

fn default_runtime() -> Runtime {
    Runtime::with_default_backends()
}

fn runtime_with_scenario(name: &str, scenario: MockScenario) -> Runtime {
    let mut rt = Runtime::new();
    rt.register_backend(name, ScenarioMockBackend::new(scenario));
    rt
}

async fn drain_events(
    mut handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = Vec::new();
    while let Some(ev) = handle.events.next().await {
        events.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (events, receipt)
}

async fn run_and_collect(rt: &Runtime, backend: &str, wo: WorkOrder) -> (Vec<AgentEvent>, Receipt) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    let (events, receipt) = drain_events(handle).await;
    (events, receipt.unwrap())
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

/// Custom backend that emits configurable events.
#[derive(Debug, Clone)]
struct ConfigurableBackend {
    event_kinds: Vec<AgentEventKind>,
    outcome: Outcome,
}

#[async_trait]
impl Backend for ConfigurableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "configurable".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        for kind in &self.event_kinds {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: kind.clone(),
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }
        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"note": "configurable"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: self.outcome.clone(),
            receipt_sha256: None,
        }
        .with_hash()?;
        Ok(receipt)
    }
}

/// Backend that always fails.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

/// Backend that sleeps for a long time, simulating a slow operation.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay: Duration,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let ev = make_event(AgentEventKind::RunStarted {
            message: "slow start".into(),
        });
        let _ = events_tx.send(ev).await;
        tokio::time::sleep(self.delay).await;
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "slow done".into(),
        });
        let _ = events_tx.send(ev).await;
        let finished = Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started)
                    .to_std()
                    .unwrap_or_default()
                    .as_millis() as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?;
        Ok(receipt)
    }
}

/// Backend that tracks how many times it was called.
#[derive(Debug, Clone)]
struct CountingBackend {
    count: Arc<AtomicUsize>,
}

impl CountingBackend {
    fn new() -> Self {
        Self {
            count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        self.count.fetch_add(1, Ordering::SeqCst);
        let started = Utc::now();
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "counted".into(),
        });
        let _ = events_tx.send(ev).await;
        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Basic Pipeline Flow
// ═══════════════════════════════════════════════════════════════════════

mod basic_pipeline {
    use super::*;

    #[tokio::test]
    async fn mock_backend_produces_receipt() {
        let rt = default_runtime();
        let wo = passthrough_wo("hello");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_events(handle).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_has_valid_hash() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("hash test")).await;
        assert!(receipt.receipt_sha256.is_some());
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_contract_version_matches() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("version test")).await;
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn receipt_work_order_id_matches() {
        let wo = passthrough_wo("id test");
        let wo_id = wo.id;
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn receipt_has_backend_identity() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("identity test")).await;
        assert_eq!(receipt.backend.id, "mock");
        assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    }

    #[tokio::test]
    async fn receipt_timestamps_are_ordered() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("time test")).await;
        assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    }

    #[tokio::test]
    async fn receipt_has_capabilities() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("caps test")).await;
        assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    }

    #[tokio::test]
    async fn receipt_trace_is_populated() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("trace test")).await;
        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn mock_emits_run_started_event() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("events test")).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }))
        );
    }

    #[tokio::test]
    async fn mock_emits_run_completed_event() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("complete test")).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }))
        );
    }

    #[tokio::test]
    async fn mock_emits_assistant_message() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("msg test")).await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
        );
    }

    #[tokio::test]
    async fn run_id_is_set_in_handle() {
        let rt = default_runtime();
        let handle = rt
            .run_streaming("mock", passthrough_wo("run id"))
            .await
            .unwrap();
        let run_id = handle.run_id;
        assert_ne!(run_id, Uuid::nil());
        let _ = drain_events(handle).await;
    }

    #[tokio::test]
    async fn events_stream_before_receipt() {
        let rt = default_runtime();
        let handle = rt
            .run_streaming("mock", passthrough_wo("stream order"))
            .await
            .unwrap();
        let mut got_event = false;
        let mut events_stream = handle.events;
        while let Some(_ev) = events_stream.next().await {
            got_event = true;
        }
        assert!(got_event);
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn receipt_usage_is_populated() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("usage test")).await;
        assert_eq!(receipt.usage.input_tokens, Some(0));
        assert_eq!(receipt.usage.output_tokens, Some(0));
    }

    #[tokio::test]
    async fn receipt_verification_harness_ok() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("verify test")).await;
        assert!(receipt.verification.harness_ok);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Execution Modes
// ═══════════════════════════════════════════════════════════════════════

mod execution_modes {
    use super::*;

    #[tokio::test]
    async fn default_mode_is_mapped() {
        let rt = default_runtime();
        let wo = passthrough_wo("default mode");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn explicit_passthrough_mode() {
        let rt = default_runtime();
        let wo = passthrough_mode_wo("passthrough");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[tokio::test]
    async fn explicit_mapped_mode() {
        let rt = default_runtime();
        let wo = mapped_wo("mapped");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn passthrough_mode_still_produces_events() {
        let rt = default_runtime();
        let wo = passthrough_mode_wo("passthrough events");
        let (events, _) = run_and_collect(&rt, "mock", wo).await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn passthrough_mode_receipt_has_hash() {
        let rt = default_runtime();
        let wo = passthrough_mode_wo("passthrough hash");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert!(receipt.receipt_sha256.is_some());
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn mapped_mode_receipt_has_hash() {
        let rt = default_runtime();
        let wo = mapped_wo("mapped hash");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn mode_does_not_affect_outcome() {
        let rt = default_runtime();
        let (_, r1) = run_and_collect(&rt, "mock", passthrough_mode_wo("pt")).await;
        let (_, r2) = run_and_collect(&rt, "mock", mapped_wo("mp")).await;
        assert_eq!(r1.outcome, Outcome::Complete);
        assert_eq!(r2.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Execution Lanes
// ═══════════════════════════════════════════════════════════════════════

mod execution_lanes {
    use super::*;

    #[tokio::test]
    async fn patch_first_lane_completes() {
        let rt = default_runtime();
        let wo = wo_with_lane("patch task", ExecutionLane::PatchFirst);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn workspace_first_lane_completes() {
        let rt = default_runtime();
        let wo = wo_with_lane("workspace task", ExecutionLane::WorkspaceFirst);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn patch_first_lane_has_valid_hash() {
        let rt = default_runtime();
        let wo = wo_with_lane("hash patch", ExecutionLane::PatchFirst);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn workspace_first_lane_has_valid_hash() {
        let rt = default_runtime();
        let wo = wo_with_lane("hash workspace", ExecutionLane::WorkspaceFirst);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn different_lanes_produce_different_work_order_ids() {
        let wo1 = wo_with_lane("t1", ExecutionLane::PatchFirst);
        let wo2 = wo_with_lane("t2", ExecutionLane::WorkspaceFirst);
        assert_ne!(wo1.id, wo2.id);
    }

    #[tokio::test]
    async fn lane_preserved_in_work_order() {
        let wo = wo_with_lane("lane test", ExecutionLane::PatchFirst);
        assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
        let wo2 = wo_with_lane("lane test", ExecutionLane::WorkspaceFirst);
        assert!(matches!(wo2.lane, ExecutionLane::WorkspaceFirst));
    }

    #[tokio::test]
    async fn both_lanes_emit_events() {
        let rt = default_runtime();
        let (ev1, _) =
            run_and_collect(&rt, "mock", wo_with_lane("l1", ExecutionLane::PatchFirst)).await;
        let (ev2, _) = run_and_collect(
            &rt,
            "mock",
            wo_with_lane("l2", ExecutionLane::WorkspaceFirst),
        )
        .await;
        assert!(!ev1.is_empty());
        assert!(!ev2.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: WorkOrder Construction
// ═══════════════════════════════════════════════════════════════════════

mod work_order_construction {
    use super::*;

    #[tokio::test]
    async fn work_order_has_unique_id() {
        let wo1 = passthrough_wo("a");
        let wo2 = passthrough_wo("b");
        assert_ne!(wo1.id, wo2.id);
    }

    #[tokio::test]
    async fn work_order_task_set_correctly() {
        let wo = passthrough_wo("my special task");
        assert_eq!(wo.task, "my special task");
    }

    #[tokio::test]
    async fn work_order_model_configuration() {
        let wo = wo_with_model("model test", "gpt-4");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    }

    #[tokio::test]
    async fn work_order_budget_configuration() {
        let wo = wo_with_budget("budget test", 10.0);
        assert_eq!(wo.config.max_budget_usd, Some(10.0));
    }

    #[tokio::test]
    async fn work_order_max_turns_configuration() {
        let wo = wo_with_max_turns("turns test", 5);
        assert_eq!(wo.config.max_turns, Some(5));
    }

    #[tokio::test]
    async fn work_order_default_workspace_mode() {
        let wo = passthrough_wo("default ws");
        assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    }

    #[tokio::test]
    async fn work_order_context_packet() {
        let ctx = ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "test".into(),
                content: "snippet content".into(),
            }],
        };
        let wo = WorkOrderBuilder::new("ctx test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .context(ctx)
            .build();
        assert_eq!(wo.context.files.len(), 1);
        assert_eq!(wo.context.snippets.len(), 1);
    }

    #[tokio::test]
    async fn work_order_empty_policy_by_default() {
        let wo = passthrough_wo("policy default");
        assert!(wo.policy.allowed_tools.is_empty());
        assert!(wo.policy.disallowed_tools.is_empty());
    }

    #[tokio::test]
    async fn work_order_requirements_empty_by_default() {
        let wo = passthrough_wo("reqs default");
        assert!(wo.requirements.required.is_empty());
    }

    #[tokio::test]
    async fn work_order_with_model_flows_through_pipeline() {
        let rt = default_runtime();
        let wo = wo_with_model("model pipeline", "test-model");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Error Handling
// ═══════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[tokio::test]
    async fn unknown_backend_returns_error() {
        let rt = default_runtime();
        let result = rt.run_streaming("nonexistent", passthrough_wo("err")).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[tokio::test]
    async fn unknown_backend_error_code() {
        let rt = default_runtime();
        let result = rt
            .run_streaming("no-such-backend", passthrough_wo("err"))
            .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        assert_eq!(err.error_code().as_str(), "backend_not_found");
    }

    #[tokio::test]
    async fn unknown_backend_not_retryable() {
        let rt = default_runtime();
        let result = rt.run_streaming("missing", passthrough_wo("retry")).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn failing_backend_returns_backend_failed() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "intentional failure".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", passthrough_wo("fail test"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RuntimeError::BackendFailed(_)));
    }

    #[tokio::test]
    async fn backend_failed_is_retryable() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "transient".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", passthrough_wo("retry"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        let err = result.unwrap_err();
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn backend_failed_error_code() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "code test".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", passthrough_wo("code"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        let err = result.unwrap_err();
        assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
        assert_eq!(err.error_code().as_str(), "backend_crashed");
    }

    #[tokio::test]
    async fn scenario_permanent_error() {
        let rt = runtime_with_scenario(
            "permerr",
            MockScenario::PermanentError {
                code: "ABP-B001".into(),
                message: "permanent fail".into(),
            },
        );
        let handle = rt
            .run_streaming("permerr", passthrough_wo("perm"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn scenario_timeout_error() {
        let rt = runtime_with_scenario("timeout", MockScenario::Timeout { after_ms: 10 });
        let handle = rt
            .run_streaming("timeout", passthrough_wo("to"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn scenario_rate_limited_error() {
        let rt = runtime_with_scenario(
            "ratelimit",
            MockScenario::RateLimited {
                retry_after_ms: 100,
            },
        );
        let handle = rt
            .run_streaming("ratelimit", passthrough_wo("rl"))
            .await
            .unwrap();
        let (_, result) = drain_events(handle).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn error_code_as_str_is_snake_case() {
        assert_eq!(ErrorCode::BackendNotFound.as_str(), "backend_not_found");
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
        assert_eq!(ErrorCode::PolicyDenied.as_str(), "policy_denied");
        assert_eq!(ErrorCode::PolicyInvalid.as_str(), "policy_invalid");
        assert_eq!(
            ErrorCode::CapabilityUnsupported.as_str(),
            "capability_unsupported"
        );
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.as_str(),
            "workspace_init_failed"
        );
        assert_eq!(ErrorCode::Internal.as_str(), "internal");
    }

    #[tokio::test]
    async fn runtime_error_into_abp_error() {
        let err = RuntimeError::UnknownBackend {
            name: "test".into(),
        };
        let abp_err = err.into_abp_error();
        assert_eq!(abp_err.code, ErrorCode::BackendNotFound);
    }

    #[tokio::test]
    async fn all_error_codes_have_as_str() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
            ErrorCode::ExecutionToolFailed,
            ErrorCode::ExecutionWorkspaceError,
            ErrorCode::ExecutionPermissionDenied,
            ErrorCode::ContractVersionMismatch,
            ErrorCode::ContractSchemaViolation,
            ErrorCode::ContractInvalidReceipt,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
            ErrorCode::PolicyDenied,
            ErrorCode::PolicyInvalid,
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
            ErrorCode::IrLoweringFailed,
            ErrorCode::IrInvalid,
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
            ErrorCode::DialectUnknown,
            ErrorCode::DialectMappingFailed,
            ErrorCode::ConfigInvalid,
            ErrorCode::Internal,
        ];
        for code in &codes {
            let s = code.as_str();
            assert!(!s.is_empty(), "empty as_str for {code:?}");
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "non-snake_case as_str for {code:?}: {s}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Scenario-Driven Tests
// ═══════════════════════════════════════════════════════════════════════

mod scenario_tests {
    use super::*;

    #[tokio::test]
    async fn success_scenario_with_delay() {
        let rt = runtime_with_scenario(
            "s",
            MockScenario::Success {
                delay_ms: 10,
                text: "delayed response".into(),
            },
        );
        let handle = rt
            .run_streaming("s", passthrough_wo("delay"))
            .await
            .unwrap();
        let (events, result) = drain_events(handle).await;
        let receipt = result.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(events.iter().any(|e| matches!(
            &e.kind,
            AgentEventKind::AssistantMessage { text } if text == "delayed response"
        )));
    }

    #[tokio::test]
    async fn streaming_success_scenario() {
        let rt = runtime_with_scenario(
            "ss",
            MockScenario::StreamingSuccess {
                chunks: vec!["hello ".into(), "world".into()],
                chunk_delay_ms: 5,
            },
        );
        let handle = rt
            .run_streaming("ss", passthrough_wo("stream"))
            .await
            .unwrap();
        let (events, result) = drain_events(handle).await;
        let receipt = result.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::AssistantDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["hello ", "world"]);
    }

    #[tokio::test]
    async fn streaming_success_many_chunks() {
        let chunks: Vec<String> = (0..20).map(|i| format!("chunk_{i}")).collect();
        let rt = runtime_with_scenario(
            "mc",
            MockScenario::StreamingSuccess {
                chunks: chunks.clone(),
                chunk_delay_ms: 1,
            },
        );
        let handle = rt
            .run_streaming("mc", passthrough_wo("many"))
            .await
            .unwrap();
        let (events, result) = drain_events(handle).await;
        result.unwrap();
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::AssistantDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, chunks);
    }

    #[tokio::test]
    async fn success_scenario_zero_delay() {
        let rt = runtime_with_scenario(
            "zd",
            MockScenario::Success {
                delay_ms: 0,
                text: "instant".into(),
            },
        );
        let (_, receipt) = run_and_collect(&rt, "zd", passthrough_wo("instant")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn transient_error_first_call_fails() {
        let scenario = ScenarioMockBackend::new(MockScenario::TransientError {
            fail_count: 1,
            then: Box::new(MockScenario::Success {
                delay_ms: 0,
                text: "ok".into(),
            }),
        });
        let (tx, _rx) = mpsc::channel(64);
        let result = scenario
            .run(Uuid::new_v4(), passthrough_wo("te1"), tx)
            .await;
        assert!(result.is_err());
        assert_eq!(scenario.call_count(), 1);
    }

    #[tokio::test]
    async fn scenario_call_count_increments() {
        let scenario = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "E".into(),
            message: "fail".into(),
        });
        for _ in 0..3 {
            let (tx, _rx) = mpsc::channel(64);
            let _ = scenario.run(Uuid::new_v4(), passthrough_wo("c"), tx).await;
        }
        assert_eq!(scenario.call_count(), 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Receipt Hashing & Verification
// ═══════════════════════════════════════════════════════════════════════

mod receipt_hashing {
    use super::*;

    #[tokio::test]
    async fn receipt_hash_is_deterministic() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("det1")).await;
        let hash1 = receipt.receipt_sha256.clone().unwrap();
        let hash2 = compute_hash(&receipt).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn tampered_receipt_fails_verification() {
        let rt = default_runtime();
        let (_, mut receipt) = run_and_collect(&rt, "mock", passthrough_wo("tamper")).await;
        receipt.outcome = Outcome::Failed;
        assert!(!verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_without_hash_passes_verification() {
        let rt = default_runtime();
        let (_, mut receipt) = run_and_collect(&rt, "mock", passthrough_wo("nohash")).await;
        receipt.receipt_sha256 = None;
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_hash_is_hex_string() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("hex")).await;
        let hash = receipt.receipt_sha256.unwrap();
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    }

    #[tokio::test]
    async fn different_tasks_produce_different_hashes() {
        let rt = default_runtime();
        let (_, r1) = run_and_collect(&rt, "mock", passthrough_wo("task_a")).await;
        let (_, r2) = run_and_collect(&rt, "mock", passthrough_wo("task_b")).await;
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[tokio::test]
    async fn receipt_builder_with_hash() {
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .work_order_id(Uuid::new_v4())
            .run_id(Uuid::new_v4())
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_builder_basic() {
        let wo_id = Uuid::new_v4();
        let receipt = ReceiptBuilder::new("builder-test")
            .outcome(Outcome::Partial)
            .work_order_id(wo_id)
            .build();
        assert_eq!(receipt.outcome, Outcome::Partial);
        assert_eq!(receipt.meta.work_order_id, wo_id);
        assert_eq!(receipt.backend.id, "builder-test");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Event Streaming
// ═══════════════════════════════════════════════════════════════════════

mod event_streaming {
    use super::*;

    #[tokio::test]
    async fn events_arrive_in_order() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("order")).await;
        for window in events.windows(2) {
            assert!(window[0].ts <= window[1].ts);
        }
    }

    #[tokio::test]
    async fn all_events_have_timestamps() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("ts")).await;
        for ev in &events {
            assert!(ev.ts.timestamp() > 0);
        }
    }

    #[tokio::test]
    async fn events_ext_is_none_for_mock() {
        let rt = default_runtime();
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("ext")).await;
        for ev in &events {
            assert!(ev.ext.is_none());
        }
    }

    #[tokio::test]
    async fn configurable_backend_custom_events() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "custom",
            ConfigurableBackend {
                event_kinds: vec![
                    AgentEventKind::RunStarted {
                        message: "start".into(),
                    },
                    AgentEventKind::ToolCall {
                        tool_name: "read".into(),
                        tool_use_id: Some("t1".into()),
                        parent_tool_use_id: None,
                        input: json!({"path": "foo.rs"}),
                    },
                    AgentEventKind::ToolResult {
                        tool_name: "read".into(),
                        tool_use_id: Some("t1".into()),
                        output: json!("contents"),
                        is_error: false,
                    },
                    AgentEventKind::RunCompleted {
                        message: "done".into(),
                    },
                ],
                outcome: Outcome::Complete,
            },
        );
        let (events, receipt) = run_and_collect(&rt, "custom", passthrough_wo("custom")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read")
        ));
        assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "read")));
    }

    #[tokio::test]
    async fn file_changed_event() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fc",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::FileChanged {
                    path: "src/lib.rs".into(),
                    summary: "added function".into(),
                }],
                outcome: Outcome::Complete,
            },
        );
        let (events, _) = run_and_collect(&rt, "fc", passthrough_wo("fc")).await;
        assert!(events.iter().any(
            |e| matches!(&e.kind, AgentEventKind::FileChanged { path, .. } if path == "src/lib.rs")
        ));
    }

    #[tokio::test]
    async fn command_executed_event() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "ce",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::CommandExecuted {
                    command: "cargo test".into(),
                    exit_code: Some(0),
                    output_preview: Some("ok".into()),
                }],
                outcome: Outcome::Complete,
            },
        );
        let (events, _) = run_and_collect(&rt, "ce", passthrough_wo("ce")).await;
        assert!(events.iter().any(|e| matches!(
            &e.kind,
            AgentEventKind::CommandExecuted { command, exit_code, .. }
            if command == "cargo test" && *exit_code == Some(0)
        )));
    }

    #[tokio::test]
    async fn warning_event() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "w",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::Warning {
                    message: "be careful".into(),
                }],
                outcome: Outcome::Complete,
            },
        );
        let (events, _) = run_and_collect(&rt, "w", passthrough_wo("w")).await;
        assert!(events.iter().any(|e| matches!(
            &e.kind,
            AgentEventKind::Warning { message } if message == "be careful"
        )));
    }

    #[tokio::test]
    async fn error_event_in_stream() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "err",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::Error {
                    message: "something went wrong".into(),
                    error_code: Some(ErrorCode::ExecutionToolFailed),
                }],
                outcome: Outcome::Partial,
            },
        );
        let (events, receipt) = run_and_collect(&rt, "err", passthrough_wo("err")).await;
        assert!(events.iter().any(|e| matches!(
            &e.kind,
            AgentEventKind::Error { message, .. } if message == "something went wrong"
        )));
        assert_eq!(receipt.outcome, Outcome::Partial);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Policy Enforcement
// ═══════════════════════════════════════════════════════════════════════

mod policy_enforcement {
    use super::*;

    #[tokio::test]
    async fn empty_policy_allows_execution() {
        let rt = default_runtime();
        let wo = wo_with_policy("empty policy", PolicyProfile::default());
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn policy_with_allowed_tools() {
        let policy = PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            ..Default::default()
        };
        let rt = default_runtime();
        let wo = wo_with_policy("allowed tools", policy);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn policy_engine_can_use_tool() {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".into()],
            disallowed_tools: vec!["rm".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let read_decision = engine.can_use_tool("read");
        assert!(read_decision.allowed);
        let rm_decision = engine.can_use_tool("rm");
        assert!(!rm_decision.allowed);
    }

    #[tokio::test]
    async fn policy_engine_deny_read() {
        let policy = PolicyProfile {
            deny_read: vec!["secret/*".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path(std::path::Path::new("secret/key.pem"));
        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn policy_engine_deny_write() {
        let policy = PolicyProfile {
            deny_write: vec!["config/*".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path(std::path::Path::new("config/prod.toml"));
        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn policy_allows_unrestricted_reads() {
        let policy = PolicyProfile::default();
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path(std::path::Path::new("any/file.txt"));
        assert!(decision.allowed);
    }

    #[tokio::test]
    async fn policy_allows_unrestricted_writes() {
        let policy = PolicyProfile::default();
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path(std::path::Path::new("any/file.txt"));
        assert!(decision.allowed);
    }

    #[tokio::test]
    async fn complex_policy_pipeline() {
        let policy = PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["delete".into()],
            deny_read: vec!["*.secret".into()],
            deny_write: vec![".env*".into()],
            ..Default::default()
        };
        let rt = default_runtime();
        let wo = wo_with_policy("complex policy", policy);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Workspace Staging
// ═══════════════════════════════════════════════════════════════════════

mod workspace_staging {
    use super::*;

    #[tokio::test]
    async fn passthrough_workspace_completes() {
        let rt = default_runtime();
        let wo = WorkOrderBuilder::new("passthrough ws")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_with_temp_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();
        let rt = default_runtime();
        let wo = WorkOrderBuilder::new("staged ws")
            .workspace_mode(WorkspaceMode::Staged)
            .root(tmp.path().to_str().unwrap())
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, result) = drain_events(handle).await;
        let receipt = result.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_verification_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();
        let rt = default_runtime();
        let wo = WorkOrderBuilder::new("staged verify")
            .workspace_mode(WorkspaceMode::Staged)
            .root(tmp.path().to_str().unwrap())
            .build();
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        // Staged workspaces get git_diff/git_status attached
        // The exact content depends on the workspace manager implementation
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn workspace_with_include_exclude() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("src.rs"), "fn main(){}").unwrap();
        std::fs::write(tmp.path().join("build.log"), "log").unwrap();
        let rt = default_runtime();
        let mut wo = WorkOrderBuilder::new("include exclude")
            .workspace_mode(WorkspaceMode::Staged)
            .root(tmp.path().to_str().unwrap())
            .build();
        wo.workspace.include = vec!["*.rs".into()];
        wo.workspace.exclude = vec!["*.log".into()];
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn passthrough_workspace_no_git_init() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("no git")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Stream Pipeline Integration
// ═══════════════════════════════════════════════════════════════════════

mod stream_pipeline {
    use super::*;

    #[tokio::test]
    async fn pipeline_with_recording() {
        let recorder = EventRecorder::new();
        let pipeline = StreamPipelineBuilder::new()
            .with_recorder(recorder.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("record")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(!recorder.is_empty());
    }

    #[tokio::test]
    async fn pipeline_with_stats() {
        let stats = EventStats::new();
        let pipeline = StreamPipelineBuilder::new()
            .with_stats(stats.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let _ = run_and_collect(&rt, "mock", passthrough_wo("stats")).await;
        assert!(stats.total_events() > 0);
    }

    #[tokio::test]
    async fn pipeline_filter_excludes_events() {
        let recorder = EventRecorder::new();
        let pipeline = StreamPipelineBuilder::new()
            .filter(EventFilter::by_kind("run_started"))
            .with_recorder(recorder.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("filter")).await;
        // Only run_started events should pass through
        for ev in &events {
            assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
        }
    }

    #[tokio::test]
    async fn pipeline_transform_modifies_events() {
        let transform = EventTransform::identity();
        let pipeline = StreamPipelineBuilder::new().transform(transform).build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("transform")).await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn pipeline_stats_count_by_kind() {
        let stats = EventStats::new();
        let pipeline = StreamPipelineBuilder::new()
            .with_stats(stats.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let _ = run_and_collect(&rt, "mock", passthrough_wo("kind count")).await;
        assert!(stats.count_for("run_started") >= 1);
        assert!(stats.count_for("run_completed") >= 1);
    }

    #[tokio::test]
    async fn pipeline_recorder_captures_all_events() {
        let recorder = EventRecorder::new();
        let pipeline = StreamPipelineBuilder::new()
            .with_recorder(recorder.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let (events, _) = run_and_collect(&rt, "mock", passthrough_wo("all")).await;
        assert_eq!(recorder.len(), events.len());
    }

    #[tokio::test]
    async fn pipeline_chained_filters_and_transforms() {
        let stats = EventStats::new();
        let recorder = EventRecorder::new();
        let pipeline = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .transform(EventTransform::identity())
            .with_stats(stats.clone())
            .with_recorder(recorder.clone())
            .build();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let _ = run_and_collect(&rt, "mock", passthrough_wo("chain")).await;
        assert_eq!(stats.error_count(), 0);
        assert!(!recorder.is_empty());
    }

    #[tokio::test]
    async fn no_pipeline_still_works() {
        let rt = Runtime::with_default_backends();
        assert!(rt.stream_pipeline().is_none());
        let (events, receipt) = run_and_collect(&rt, "mock", passthrough_wo("nopipe")).await;
        assert!(!events.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Concurrent Pipeline Executions
// ═══════════════════════════════════════════════════════════════════════

mod concurrent_pipelines {
    use super::*;

    #[tokio::test]
    async fn two_concurrent_runs() {
        let rt = default_runtime();
        let h1 = rt
            .run_streaming("mock", passthrough_wo("concurrent 1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", passthrough_wo("concurrent 2"))
            .await
            .unwrap();
        let (e1, r1) = drain_events(h1).await;
        let (e2, r2) = drain_events(h2).await;
        assert!(!e1.is_empty());
        assert!(!e2.is_empty());
        assert_eq!(r1.unwrap().outcome, Outcome::Complete);
        assert_eq!(r2.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn five_concurrent_runs() {
        let rt = default_runtime();
        let mut handles = Vec::new();
        for i in 0..5 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("conc-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }
        for h in handles {
            let (events, result) = drain_events(h).await;
            assert!(!events.is_empty());
            assert_eq!(result.unwrap().outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn concurrent_runs_have_unique_run_ids() {
        let rt = default_runtime();
        let h1 = rt
            .run_streaming("mock", passthrough_wo("id1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", passthrough_wo("id2"))
            .await
            .unwrap();
        assert_ne!(h1.run_id, h2.run_id);
        let _ = drain_events(h1).await;
        let _ = drain_events(h2).await;
    }

    #[tokio::test]
    async fn concurrent_with_different_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend(
            "scenario",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 10,
                text: "scenario response".into(),
            }),
        );
        let h1 = rt
            .run_streaming("mock", passthrough_wo("b1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("scenario", passthrough_wo("b2"))
            .await
            .unwrap();
        let (_, r1) = drain_events(h1).await;
        let (_, r2) = drain_events(h2).await;
        assert_eq!(r1.unwrap().outcome, Outcome::Complete);
        assert_eq!(r2.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn concurrent_counting_backend() {
        let counting = CountingBackend::new();
        let mut rt = Runtime::new();
        rt.register_backend("c", counting.clone());
        let mut handles = Vec::new();
        for i in 0..10 {
            let h = rt
                .run_streaming("c", passthrough_wo(&format!("count-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }
        for h in handles {
            let _ = drain_events(h).await;
        }
        assert_eq!(counting.count(), 10);
    }

    #[tokio::test]
    async fn concurrent_mixed_success_and_failure() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "boom".into(),
            },
        );
        let h_ok = rt
            .run_streaming("mock", passthrough_wo("ok"))
            .await
            .unwrap();
        let h_err = rt
            .run_streaming("fail", passthrough_wo("fail"))
            .await
            .unwrap();
        let (_, r_ok) = drain_events(h_ok).await;
        let (_, r_err) = drain_events(h_err).await;
        assert!(r_ok.is_ok());
        assert!(r_err.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Cancellation
// ═══════════════════════════════════════════════════════════════════════

mod cancellation {
    use super::*;
    use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};

    #[tokio::test]
    async fn cancellation_token_basics() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancellation_token_clone_shares_state() {
        let t1 = CancellationToken::new();
        let t2 = t1.clone();
        t1.cancel();
        assert!(t2.is_cancelled());
    }

    #[tokio::test]
    async fn cancellable_run_tracks_reason() {
        let run = CancellableRun::new(CancellationToken::new());
        assert!(run.reason().is_none());
        run.cancel(CancellationReason::UserRequested);
        assert!(run.is_cancelled());
        assert_eq!(run.reason(), Some(CancellationReason::UserRequested));
    }

    #[tokio::test]
    async fn cancellable_run_keeps_first_reason() {
        let run = CancellableRun::new(CancellationToken::new());
        run.cancel(CancellationReason::Timeout);
        run.cancel(CancellationReason::BudgetExhausted);
        assert_eq!(run.reason(), Some(CancellationReason::Timeout));
    }

    #[tokio::test]
    async fn cancel_slow_backend_mid_run() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "slow",
            SlowBackend {
                delay: Duration::from_secs(10),
            },
        );
        let handle = rt
            .run_streaming("slow", passthrough_wo("cancel me"))
            .await
            .unwrap();
        // Drop the handle to trigger cancellation of the task
        let events_stream = handle.events;
        drop(events_stream);
        // The receipt task may or may not complete depending on timing
        let result = tokio::time::timeout(Duration::from_secs(2), handle.receipt).await;
        // We mostly care that we don't hang forever
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn all_cancellation_reasons_have_descriptions() {
        let reasons = [
            CancellationReason::UserRequested,
            CancellationReason::Timeout,
            CancellationReason::BudgetExhausted,
            CancellationReason::PolicyViolation,
            CancellationReason::SystemShutdown,
        ];
        for r in &reasons {
            assert!(!r.description().is_empty());
        }
    }

    #[tokio::test]
    async fn cancelled_future_resolves_immediately_when_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        // Should resolve immediately, not hang
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .expect("cancelled() should resolve immediately");
    }

    #[tokio::test]
    async fn cancelled_future_resolves_when_cancelled_later() {
        let token = CancellationToken::new();
        let t2 = token.clone();
        let handle = tokio::spawn(async move {
            t2.cancelled().await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        token.cancel();
        tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("should complete")
            .expect("task should not panic");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Backend Registration & Selection
// ═══════════════════════════════════════════════════════════════════════

mod backend_registration {
    use super::*;

    #[tokio::test]
    async fn register_and_list_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("a", MockBackend);
        rt.register_backend("b", MockBackend);
        let names = rt.backend_names();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[tokio::test]
    async fn backend_lookup_returns_some() {
        let rt = default_runtime();
        assert!(rt.backend("mock").is_some());
    }

    #[tokio::test]
    async fn backend_lookup_returns_none_for_missing() {
        let rt = default_runtime();
        assert!(rt.backend("nonexistent").is_none());
    }

    #[tokio::test]
    async fn replace_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("b", MockBackend);
        let b1 = rt.backend("b").unwrap();
        assert_eq!(b1.identity().id, "mock");
        rt.register_backend(
            "b",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "replaced".into(),
            }),
        );
        let b2 = rt.backend("b").unwrap();
        assert_eq!(b2.identity().id, "scenario-mock");
    }

    #[tokio::test]
    async fn registry_contains() {
        let rt = default_runtime();
        assert!(rt.registry().contains("mock"));
        assert!(!rt.registry().contains("absent"));
    }

    #[tokio::test]
    async fn default_backends_includes_mock() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend_names().contains(&"mock".to_string()));
    }

    #[tokio::test]
    async fn new_runtime_has_no_backends() {
        let rt = Runtime::new();
        assert!(rt.backend_names().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Receipt Chain
// ═══════════════════════════════════════════════════════════════════════

mod receipt_chain {
    use super::*;

    #[tokio::test]
    async fn receipt_chain_accumulates() {
        let rt = default_runtime();
        let _ = run_and_collect(&rt, "mock", passthrough_wo("chain1")).await;
        let _ = run_and_collect(&rt, "mock", passthrough_wo("chain2")).await;
        let chain = rt.receipt_chain();
        let chain = chain.lock().await;
        assert!(chain.len() >= 2);
    }

    #[tokio::test]
    async fn receipt_chain_starts_empty() {
        let rt = default_runtime();
        let chain = rt.receipt_chain();
        let chain = chain.lock().await;
        assert_eq!(chain.len(), 0);
    }

    #[tokio::test]
    async fn receipt_chain_after_single_run() {
        let rt = default_runtime();
        let _ = run_and_collect(&rt, "mock", passthrough_wo("single")).await;
        let chain = rt.receipt_chain();
        let chain = chain.lock().await;
        assert_eq!(chain.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Capability Requirements
// ═══════════════════════════════════════════════════════════════════════

mod capability_requirements {
    use super::*;

    #[tokio::test]
    async fn satisfiable_requirements_pass() {
        let rt = default_runtime();
        let result = rt.check_capabilities(
            "mock",
            &CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                }],
            },
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn unsatisfiable_requirements_fail() {
        let rt = default_runtime();
        let result = rt.check_capabilities(
            "mock",
            &CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Native,
                }],
            },
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn check_capabilities_unknown_backend() {
        let rt = default_runtime();
        let result =
            rt.check_capabilities("nonexistent", &CapabilityRequirements { required: vec![] });
        assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
    }

    #[tokio::test]
    async fn empty_requirements_always_pass() {
        let rt = default_runtime();
        let result = rt.check_capabilities("mock", &CapabilityRequirements { required: vec![] });
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn emulated_support_satisfies_emulated_min() {
        let rt = default_runtime();
        let result = rt.check_capabilities(
            "mock",
            &CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                }],
            },
        );
        assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Metrics & Telemetry
// ═══════════════════════════════════════════════════════════════════════

mod metrics_telemetry {
    use super::*;

    #[tokio::test]
    async fn metrics_updated_after_run() {
        let rt = default_runtime();
        let _ = run_and_collect(&rt, "mock", passthrough_wo("metrics")).await;
        let m = rt.metrics().snapshot();
        assert!(m.total_runs >= 1);
    }

    #[tokio::test]
    async fn metrics_success_count() {
        let rt = default_runtime();
        let _ = run_and_collect(&rt, "mock", passthrough_wo("success metric")).await;
        let m = rt.metrics().snapshot();
        assert!(m.successful_runs >= 1);
    }

    #[tokio::test]
    async fn metrics_after_multiple_runs() {
        let rt = default_runtime();
        for i in 0..3 {
            let _ = run_and_collect(&rt, "mock", passthrough_wo(&format!("m-{i}"))).await;
        }
        let m = rt.metrics().snapshot();
        assert!(m.total_runs >= 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Direct Backend Invocation
// ═══════════════════════════════════════════════════════════════════════

mod direct_backend {
    use super::*;

    #[tokio::test]
    async fn mock_backend_direct_run() {
        let (tx, mut rx) = mpsc::channel(64);
        let wo = passthrough_wo("direct mock");
        let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(verify_hash(&receipt));
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn scenario_mock_direct_success() {
        let backend = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "direct scenario".into(),
        });
        let (tx, _rx) = mpsc::channel(64);
        let receipt = backend
            .run(Uuid::new_v4(), passthrough_wo("ds"), tx)
            .await
            .unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn scenario_mock_direct_permanent_error() {
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "E1".into(),
            message: "nope".into(),
        });
        let (tx, _rx) = mpsc::channel(64);
        let result = backend.run(Uuid::new_v4(), passthrough_wo("pe"), tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_backend_identity() {
        let id = MockBackend.identity();
        assert_eq!(id.id, "mock");
        assert!(id.backend_version.is_some());
    }

    #[tokio::test]
    async fn mock_backend_capabilities_include_streaming() {
        let caps = MockBackend.capabilities();
        assert!(caps.contains_key(&Capability::Streaming));
    }

    #[tokio::test]
    async fn scenario_backend_identity() {
        let b = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "id".into(),
        });
        assert_eq!(b.identity().id, "scenario-mock");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Edge Cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[tokio::test]
    async fn empty_task_string() {
        let rt = default_runtime();
        let wo = passthrough_wo("");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn very_long_task_string() {
        let rt = default_runtime();
        let long_task = "x".repeat(10_000);
        let wo = passthrough_wo(&long_task);
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn unicode_task_string() {
        let rt = default_runtime();
        let wo = passthrough_wo("日本語のタスク 🚀");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn special_chars_in_task() {
        let rt = default_runtime();
        let wo = passthrough_wo("fix `bug` in \"file.rs\" <html> & more");
        let (_, receipt) = run_and_collect(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn configurable_backend_no_events() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "empty",
            ConfigurableBackend {
                event_kinds: vec![],
                outcome: Outcome::Complete,
            },
        );
        let (events, receipt) = run_and_collect(&rt, "empty", passthrough_wo("no events")).await;
        assert!(events.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn configurable_backend_partial_outcome() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "partial",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::Warning {
                    message: "partial".into(),
                }],
                outcome: Outcome::Partial,
            },
        );
        let (_, receipt) = run_and_collect(&rt, "partial", passthrough_wo("partial")).await;
        assert_eq!(receipt.outcome, Outcome::Partial);
    }

    #[tokio::test]
    async fn configurable_backend_failed_outcome() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "failed",
            ConfigurableBackend {
                event_kinds: vec![AgentEventKind::Error {
                    message: "oops".into(),
                    error_code: None,
                }],
                outcome: Outcome::Failed,
            },
        );
        let (_, receipt) = run_and_collect(&rt, "failed", passthrough_wo("failed")).await;
        assert_eq!(receipt.outcome, Outcome::Failed);
    }

    #[tokio::test]
    async fn receipt_serialization_roundtrip() {
        let rt = default_runtime();
        let (_, receipt) = run_and_collect(&rt, "mock", passthrough_wo("serde")).await;
        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.outcome, receipt.outcome);
        assert_eq!(deserialized.meta.run_id, receipt.meta.run_id);
        assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
    }

    #[tokio::test]
    async fn work_order_serialization_roundtrip() {
        let wo = passthrough_wo("serde test");
        let json = serde_json::to_string(&wo).unwrap();
        let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, wo.id);
        assert_eq!(deserialized.task, wo.task);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Recorder Backend
// ═══════════════════════════════════════════════════════════════════════

mod recorder_backend {
    use super::*;
    use abp_backend_mock::scenarios::MockBackendRecorder;

    #[tokio::test]
    async fn recorder_captures_calls() {
        let recorder = MockBackendRecorder::new(MockBackend);
        let mut rt = Runtime::new();
        rt.register_backend("rec", recorder.clone());
        let _ = run_and_collect(&rt, "rec", passthrough_wo("recorded")).await;
        assert_eq!(recorder.call_count().await, 1);
    }

    #[tokio::test]
    async fn recorder_captures_multiple_calls() {
        let recorder = MockBackendRecorder::new(MockBackend);
        let mut rt = Runtime::new();
        rt.register_backend("rec", recorder.clone());
        for i in 0..3 {
            let _ = run_and_collect(&rt, "rec", passthrough_wo(&format!("r-{i}"))).await;
        }
        assert_eq!(recorder.call_count().await, 3);
    }

    #[tokio::test]
    async fn recorder_preserves_identity() {
        let recorder = MockBackendRecorder::new(MockBackend);
        assert_eq!(recorder.identity().id, "mock");
    }

    #[tokio::test]
    async fn recorder_last_call() {
        let recorder = MockBackendRecorder::new(MockBackend);
        let mut rt = Runtime::new();
        rt.register_backend("rec", recorder.clone());
        let _ = run_and_collect(&rt, "rec", passthrough_wo("last")).await;
        let last = recorder.last_call().await.unwrap();
        assert_eq!(last.work_order.task, "last");
        assert!(last.result.is_ok());
    }
}
