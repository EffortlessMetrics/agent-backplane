#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
//! Exhaustive end-to-end pipeline integration tests.
//!
//! Categories:
//! 1. Full pipeline execution (10): WorkOrder → Backend → Events → Receipt
//! 2. Pipeline with middleware (10): Logging, telemetry, policy middlewares
//! 3. Pipeline with capability negotiation (10): Capability checks before execution
//! 4. Pipeline with workspace staging (10): Workspace create / verify / clean
//! 5. Pipeline error handling (10+): Backend failure, invalid work order, timeout

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
use abp_receipt::{ReceiptChain, compute_hash, verify_hash};
use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker, BudgetViolation};
use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};
use abp_runtime::middleware::{
    AuditMiddleware, LoggingMiddleware, Middleware, MiddlewareChain, MiddlewareContext,
    PolicyMiddleware, TelemetryMiddleware,
};
use abp_runtime::pipeline::{
    AuditStage, Pipeline, PipelineStage, PolicyStage, RuntimePipeline, StageOutcome,
    ValidationStage,
};
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{EventFilter, EventRecorder, EventStats, StreamPipelineBuilder};
use abp_workspace::WorkspaceStager;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("task panicked");
    (collected, receipt)
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = passthrough_wo(task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

// ---------------------------------------------------------------------------
// Custom test backends
// ---------------------------------------------------------------------------

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
        _wo: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("intentional failure")
    }
}

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
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let ev = make_event(AgentEventKind::RunStarted {
            message: "starting slow".into(),
        });
        let _ = tx.send(ev).await;
        tokio::time::sleep(self.delay).await;
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let _ = tx.send(ev).await;
        let finished = Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace: vec![],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend that emits a configurable number of events with tool calls.
#[derive(Debug, Clone)]
struct RichEventsBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for RichEventsBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "rich-events".into(),
            backend_version: Some("0.2".into()),
            adapter_version: Some("1.0".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
        m.insert(Capability::ToolWrite, SupportLevel::Native);
        m.insert(Capability::ToolUse, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();

        let ev = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        trace.push(ev.clone());
        let _ = tx.send(ev).await;

        for i in 0..self.event_count {
            let ev = make_event(AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("t{i}")),
                parent_tool_use_id: None,
                input: json!({"idx": i}),
            });
            trace.push(ev.clone());
            let _ = tx.send(ev).await;

            let ev = make_event(AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("t{i}")),
                output: json!({"ok": true}),
                is_error: false,
            });
            trace.push(ev.clone());
            let _ = tx.send(ev).await;
        }

        let ev = make_event(AgentEventKind::AssistantMessage {
            text: "All done".into(),
        });
        trace.push(ev.clone());
        let _ = tx.send(ev).await;

        let ev = make_event(AgentEventKind::RunCompleted {
            message: "finished".into(),
        });
        trace.push(ev.clone());
        let _ = tx.send(ev).await;

        let finished = Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"tool_calls": self.event_count}),
            usage: UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50 * self.event_count as u64),
                ..Default::default()
            },
            trace,
            artifacts: vec![ArtifactRef {
                kind: "patch".into(),
                path: "output.diff".into(),
            }],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend that returns a partial outcome.
#[derive(Debug, Clone)]
struct PartialBackend;

#[async_trait]
impl Backend for PartialBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial".into(),
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
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let ev = make_event(AgentEventKind::RunStarted {
            message: "partial run".into(),
        });
        let _ = tx.send(ev).await;
        let ev = make_event(AgentEventKind::Warning {
            message: "not all tasks completed".into(),
        });
        let _ = tx.send(ev).await;
        let finished = Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace: vec![],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Partial,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend with specific capabilities for negotiation tests.
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
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let ev = make_event(AgentEventKind::RunStarted {
            message: "cap run".into(),
        });
        let _ = tx.send(ev).await;
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let _ = tx.send(ev).await;
        let finished = Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace: vec![],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

// ===========================================================================
// 1. Full pipeline execution (10 tests)
// ===========================================================================

#[tokio::test]
async fn full_pipeline_basic_complete() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "basic pipeline").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn full_pipeline_receipt_has_valid_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "hash pipeline").await;
    let hash = receipt.receipt_sha256.as_ref().expect("hash must be set");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());
}

#[tokio::test]
async fn full_pipeline_contract_version_matches() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "version pipeline").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn full_pipeline_work_order_id_preserved() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("id test");
    let expected_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().meta.work_order_id, expected_id);
}

#[tokio::test]
async fn full_pipeline_timing_consistent() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "timing pipeline").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn full_pipeline_events_start_and_end() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "event ordering").await;
    let first_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(first_started, "must have RunStarted event");
    assert!(has_completed, "must have RunCompleted event");
}

#[tokio::test]
async fn full_pipeline_rich_events_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("rich", RichEventsBackend { event_count: 3 });
    let wo = passthrough_wo("rich events");
    let handle = rt.run_streaming("rich", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // 1 RunStarted + 3*(ToolCall+ToolResult) + 1 AssistantMessage + 1 RunCompleted = 9
    assert!(
        events.len() >= 9,
        "expected >=9 events, got {}",
        events.len()
    );
    assert_eq!(receipt.backend.id, "rich-events");
    assert!(receipt.artifacts.len() == 1);
}

#[tokio::test]
async fn full_pipeline_partial_outcome() {
    let mut rt = Runtime::new();
    rt.register_backend("partial", PartialBackend);
    let wo = passthrough_wo("partial test");
    let handle = rt.run_streaming("partial", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Partial);
}

#[tokio::test]
async fn full_pipeline_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain-1").await;
    run_mock(&rt, "chain-2").await;
    run_mock(&rt, "chain-3").await;
    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert!(locked.len() >= 3);
}

#[tokio::test]
async fn full_pipeline_receipt_verify_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "verify hash").await;
    assert!(verify_hash(&receipt));
    assert_eq!(
        receipt.receipt_sha256.as_ref().unwrap(),
        &compute_hash(&receipt).unwrap()
    );
}

// ===========================================================================
// 2. Pipeline with middleware (10 tests)
// ===========================================================================

#[tokio::test]
async fn middleware_logging_before_and_after() {
    let chain = MiddlewareChain::new().with(LoggingMiddleware::default());
    let wo = passthrough_wo("logging test");
    let ctx = MiddlewareContext::new("mock");
    chain.run_before(&wo, &ctx).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let errors = chain.run_after(&wo, &ctx, Some(&receipt)).await;
    assert!(errors.is_empty());
}

#[tokio::test]
async fn middleware_telemetry_records_metrics() {
    let metrics = Arc::new(RunMetrics::new());
    let chain = MiddlewareChain::new().with(TelemetryMiddleware::new(Arc::clone(&metrics)));
    let wo = passthrough_wo("telemetry test");
    let ctx = MiddlewareContext::new("mock");
    chain.run_before(&wo, &ctx).await.unwrap();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    chain.run_after(&wo, &ctx, Some(&receipt)).await;
    let snap = metrics.snapshot();
    assert!(snap.total_runs >= 1);
}

#[tokio::test]
async fn middleware_policy_allows_clean_order() {
    let chain = MiddlewareChain::new().with(PolicyMiddleware);
    let wo = passthrough_wo("clean policy");
    let ctx = MiddlewareContext::new("mock");
    let result = chain.run_before(&wo, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn middleware_policy_blocks_conflicting_tools() {
    let chain = MiddlewareChain::new().with(PolicyMiddleware);
    let wo = WorkOrderBuilder::new("conflict test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["bash".into()],
            disallowed_tools: vec!["bash".into()],
            ..Default::default()
        })
        .build();
    let ctx = MiddlewareContext::new("mock");
    let result = chain.run_before(&wo, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn middleware_chain_order_matters() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::default())
        .with(PolicyMiddleware)
        .with(LoggingMiddleware::default());
    assert_eq!(chain.len(), 3);
    let names = chain.names();
    assert_eq!(names, vec!["logging", "policy", "logging"]);
}

#[tokio::test]
async fn middleware_audit_records_work_order_ids() {
    let audit = Arc::new(AuditMiddleware::new());
    let mut chain = MiddlewareChain::new();
    // Clone Arc to keep a handle for later inspection.
    let audit_ref = Arc::clone(&audit);
    chain.push(AuditMiddlewareWrapper(audit_ref));

    let wo1 = passthrough_wo("audit-1");
    let wo2 = passthrough_wo("audit-2");
    let id1 = wo1.id;
    let id2 = wo2.id;
    let ctx = MiddlewareContext::new("mock");

    chain.run_before(&wo1, &ctx).await.unwrap();
    chain.run_before(&wo2, &ctx).await.unwrap();

    let ids = audit.ids().await;
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

/// Wrapper to use Arc<AuditMiddleware> as a Middleware impl.
struct AuditMiddlewareWrapper(Arc<AuditMiddleware>);

#[async_trait]
impl Middleware for AuditMiddlewareWrapper {
    async fn before_run(&self, order: &WorkOrder, ctx: &MiddlewareContext) -> anyhow::Result<()> {
        self.0.before_run(order, ctx).await
    }
    async fn after_run(
        &self,
        order: &WorkOrder,
        ctx: &MiddlewareContext,
        receipt: Option<&Receipt>,
    ) -> anyhow::Result<()> {
        self.0.after_run(order, ctx, receipt).await
    }
    fn name(&self) -> &str {
        "audit-wrapper"
    }
}

#[tokio::test]
async fn middleware_telemetry_records_failure() {
    let metrics = Arc::new(RunMetrics::new());
    let chain = MiddlewareChain::new().with(TelemetryMiddleware::new(Arc::clone(&metrics)));
    let wo = passthrough_wo("fail telemetry");
    let ctx = MiddlewareContext::new("mock");
    chain.run_before(&wo, &ctx).await.unwrap();
    // Pass None receipt to signal failure.
    chain.run_after(&wo, &ctx, None).await;
    let snap = metrics.snapshot();
    assert!(snap.total_runs >= 1);
    assert!(snap.failed_runs >= 1);
}

#[tokio::test]
async fn middleware_chain_empty_is_noop() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    let wo = passthrough_wo("noop");
    let ctx = MiddlewareContext::new("mock");
    chain.run_before(&wo, &ctx).await.unwrap();
    let errors = chain.run_after(&wo, &ctx, None).await;
    assert!(errors.is_empty());
}

#[tokio::test]
async fn middleware_full_chain_with_successful_run() {
    let metrics = Arc::new(RunMetrics::new());
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::default())
        .with(TelemetryMiddleware::new(Arc::clone(&metrics)))
        .with(PolicyMiddleware);

    let wo = passthrough_wo("full chain");
    let ctx = MiddlewareContext::new("mock");

    chain.run_before(&wo, &ctx).await.unwrap();

    // Simulate backend run.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let errors = chain.run_after(&wo, &ctx, Some(&receipt)).await;
    assert!(errors.is_empty());
    let snap = metrics.snapshot();
    assert!(snap.total_runs >= 1);
    assert!(snap.successful_runs >= 1);
}

#[tokio::test]
async fn middleware_context_elapsed_increases() {
    let ctx = MiddlewareContext::new("test-backend");
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(ctx.elapsed_ms() >= 1);
    assert_eq!(ctx.backend_name, "test-backend");
}

// ===========================================================================
// 3. Pipeline with capability negotiation (10 tests)
// ===========================================================================

#[tokio::test]
async fn capability_check_passes_empty_requirements() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    assert!(rt.check_capabilities("mock", &reqs).is_ok());
}

#[tokio::test]
async fn capability_check_fails_unsupported_native() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs);
    assert!(err.is_err());
    assert!(matches!(
        err.unwrap_err(),
        RuntimeError::CapabilityCheckFailed(_)
    ));
}

#[tokio::test]
async fn capability_check_unknown_backend_fails() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("nonexistent", &reqs);
    assert!(matches!(
        err.unwrap_err(),
        RuntimeError::UnknownBackend { .. }
    ));
}

#[tokio::test]
async fn capability_negotiation_native_caps_pass() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let mut rt = Runtime::new();
    rt.register_backend("cap-test", CapableBackend { caps });

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
        ],
    };
    assert!(rt.check_capabilities("cap-test", &reqs).is_ok());
}

#[tokio::test]
async fn capability_negotiation_emulated_with_native_min_fails() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Vision, SupportLevel::Emulated);

    let mut rt = Runtime::new();
    rt.register_backend("emu", CapableBackend { caps });

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("emu", &reqs).is_err());
}

#[tokio::test]
async fn capability_negotiation_emulated_with_emulated_min_passes() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Vision, SupportLevel::Emulated);

    let mut rt = Runtime::new();
    rt.register_backend("emu-pass", CapableBackend { caps });

    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(rt.check_capabilities("emu-pass", &reqs).is_ok());
}

#[tokio::test]
async fn capability_negotiation_multiple_missing() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "empty-caps",
        CapableBackend {
            caps: CapabilityManifest::new(),
        },
    );

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Native,
            },
        ],
    };
    // Empty manifest backends skip check (sidecar behavior).
    // Register a backend with at least one cap so the check isn't skipped.
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    let mut rt2 = Runtime::new();
    rt2.register_backend("sparse", CapableBackend { caps });

    let err = rt2.check_capabilities("sparse", &reqs);
    assert!(err.is_err());
}

#[tokio::test]
async fn capability_runtime_pipeline_negotiate_passes() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let backend: Arc<dyn Backend> = Arc::new(CapableBackend { caps });
    let pipeline = RuntimePipeline::new("capable", backend);

    let wo = passthrough_wo("negotiate pass");
    assert!(pipeline.negotiate_capabilities(&wo).is_ok());
}

#[tokio::test]
async fn capability_runtime_pipeline_negotiate_fails_strict() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let backend: Arc<dyn Backend> = Arc::new(CapableBackend { caps });
    let pipeline = RuntimePipeline::new("strict", backend);

    let wo = WorkOrderBuilder::new("strict caps")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpServer,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(pipeline.negotiate_capabilities(&wo).is_err());
}

#[tokio::test]
async fn capability_backend_identity_in_receipt() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);

    let mut rt = Runtime::new();
    rt.register_backend("cap-receipt", CapableBackend { caps: caps.clone() });

    let wo = passthrough_wo("cap receipt");
    let handle = rt.run_streaming("cap-receipt", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.backend.id, "capable");
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    assert!(receipt.capabilities.contains_key(&Capability::ToolUse));
}

// ===========================================================================
// 4. Pipeline with workspace staging (10 tests)
// ===========================================================================

#[test]
fn workspace_stager_creates_temp_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "world").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .stage()
        .unwrap();

    assert!(prepared.path().exists());
    assert!(prepared.is_staged());
}

#[test]
fn workspace_stager_with_git_init() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.rs"), "fn main() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(prepared.path().join(".git").exists());
}

#[test]
fn workspace_stager_exclude_patterns() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.txt"), "yes").unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules")).unwrap();
    std::fs::write(dir.path().join("node_modules").join("pkg.json"), "{}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .exclude(vec!["node_modules/**".into()])
        .stage()
        .unwrap();

    assert!(prepared.path().join("keep.txt").exists());
    // Files inside node_modules are excluded by the glob pattern.
    assert!(!prepared.path().join("node_modules").join("pkg.json").exists());
}

#[test]
fn workspace_metadata_reports_file_counts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .stage()
        .unwrap();

    let meta = prepared.metadata().unwrap();
    assert!(meta.file_count >= 2);
}

#[test]
fn workspace_validate_reports_valid() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("valid.rs"), "fn main() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .stage()
        .unwrap();

    let result = prepared.validate();
    assert!(result.valid);
}

#[test]
fn workspace_cleanup_removes_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("temp.txt"), "temporary").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .stage()
        .unwrap();

    let staged_path = prepared.path().to_path_buf();
    assert!(staged_path.exists());
    prepared.cleanup().unwrap();
    assert!(!staged_path.exists());
}

#[test]
fn workspace_stager_include_patterns() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.path().join("skip.txt"), "skip me").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .include(vec!["*.rs".into()])
        .stage()
        .unwrap();

    assert!(prepared.path().join("keep.rs").exists());
    // With include patterns, non-matching files should be excluded.
    assert!(!prepared.path().join("skip.txt").exists());
}

#[tokio::test]
async fn workspace_pipeline_stage_prepare() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("src.rs"), "fn main() {}").unwrap();

    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let pipeline = RuntimePipeline::new("mock", backend);

    let wo = WorkOrderBuilder::new("workspace pipeline")
        .workspace_mode(WorkspaceMode::Staged)
        .root(dir.path().to_string_lossy().to_string())
        .build();

    let prepared = pipeline.prepare_workspace(&wo);
    assert!(prepared.is_ok());
    let prep = prepared.unwrap();
    assert!(prep.path().exists());
}

#[test]
fn workspace_diff_on_fresh_staged() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("code.rs"), "fn hello() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // A fresh staged workspace with git init should have a clean diff.
    let diff = prepared.diff_summary();
    assert!(diff.is_ok());
}

#[test]
fn workspace_snapshot_captures_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("snap.txt"), "snapshot me").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(dir.path())
        .stage()
        .unwrap();

    let snap = prepared.snapshot();
    assert!(snap.is_ok());
}

// ===========================================================================
// 5. Pipeline error handling (12 tests)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend_rejected() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("unknown backend");
    let err = rt.run_streaming("nonexistent", wo).await;
    assert!(err.is_err());
    match err {
        Err(RuntimeError::UnknownBackend { .. }) => {}
        _ => panic!("expected UnknownBackend error"),
    }
}

#[tokio::test]
async fn error_failing_backend_propagates() {
    let mut rt = Runtime::new();
    rt.register_backend("fail", FailingBackend);
    let wo = passthrough_wo("fail propagation");
    let handle = rt.run_streaming("fail", wo).await.unwrap();
    let (_, result) = drain_run(handle).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::BackendFailed(_)
    ));
}

#[tokio::test]
async fn error_pipeline_validation_empty_task() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = passthrough_wo("");
    wo.task = "".into();
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("task"));
}

#[tokio::test]
async fn error_pipeline_validation_empty_root() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = passthrough_wo("good task");
    wo.workspace.root = "".into();
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("root"));
}

#[tokio::test]
async fn error_pipeline_policy_conflicting_tools() {
    let pipeline = Pipeline::new().stage(PolicyStage);
    let mut wo = WorkOrderBuilder::new("conflict")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["read_file".into()],
            disallowed_tools: vec!["read_file".into()],
            ..Default::default()
        })
        .build();
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn error_runtime_pipeline_full_with_failing_backend() {
    let backend: Arc<dyn Backend> = Arc::new(FailingBackend);
    let pipeline = RuntimePipeline::new("failing", backend);
    let wo = passthrough_wo("runtime fail");
    let (outcomes, receipt) = pipeline.execute(wo).await;

    // Policy and capability stages should pass; backend should fail.
    let backend_stage = outcomes.iter().find(|o| o.name == "run_backend");
    assert!(backend_stage.is_some());
    assert!(!backend_stage.unwrap().success);
}

#[tokio::test]
async fn error_cancellation_token_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn error_cancellable_run_with_reason() {
    let token = CancellationToken::new();
    let run = CancellableRun::new(token.clone());
    run.cancel(CancellationReason::UserRequested);
    assert!(token.is_cancelled());
    let reason = run.reason();
    assert!(reason.is_some());
    assert!(matches!(reason.unwrap(), CancellationReason::UserRequested));
}

#[tokio::test]
async fn error_budget_tracker_detects_token_overage() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        max_cost_usd: None,
        max_turns: None,
        max_duration: None,
    });
    tracker.record_tokens(50);
    assert!(matches!(tracker.check(), BudgetStatus::WithinLimits));
    tracker.record_tokens(60);
    // Should now be over budget.
    let status = tracker.check();
    assert!(matches!(status, BudgetStatus::Exceeded(_)));
}

#[tokio::test]
async fn error_budget_tracker_detects_turn_overage() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: None,
        max_cost_usd: None,
        max_turns: Some(3),
        max_duration: None,
    });
    tracker.record_turn();
    tracker.record_turn();
    // 2/3 = 67% — under the 80% warning threshold.
    assert!(matches!(tracker.check(), BudgetStatus::WithinLimits));
    tracker.record_turn();
    tracker.record_turn();
    assert!(matches!(tracker.check(), BudgetStatus::Exceeded(_)));
}

#[tokio::test]
async fn error_runtime_error_display_contains_name() {
    let e = RuntimeError::UnknownBackend {
        name: "my-backend".into(),
    };
    assert!(e.to_string().contains("my-backend"));
}

#[tokio::test]
async fn error_runtime_error_retryability() {
    let retryable = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    let not_retryable = RuntimeError::UnknownBackend { name: "x".into() };
    let not_retryable2 = RuntimeError::CapabilityCheckFailed("missing cap".into());
    assert!(retryable.is_retryable());
    assert!(!not_retryable.is_retryable());
    assert!(!not_retryable2.is_retryable());
}

// ===========================================================================
// Bonus: Cross-cutting pipeline stages (additional 3 tests)
// ===========================================================================

#[tokio::test]
async fn pipeline_audit_stage_records_ids() {
    let audit = AuditStage::new();
    let pipeline = Pipeline::new().stage(ValidationStage);

    // Use audit stage separately to inspect.
    let mut wo = passthrough_wo("audit capture");
    audit.process(&mut wo).await.unwrap();
    let entries = audit.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].task, "audit capture");
}

#[tokio::test]
async fn pipeline_multiple_stages_chain() {
    let audit = AuditStage::new();
    let pipeline = Pipeline::new()
        .stage(ValidationStage)
        .stage(PolicyStage)
        .stage(audit);

    assert_eq!(pipeline.len(), 3);
    assert!(!pipeline.is_empty());

    let mut wo = passthrough_wo("multi stage");
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn pipeline_runtime_pipeline_full_execute() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let pipeline = RuntimePipeline::new("mock", backend);
    let wo = passthrough_wo("full execute");
    let (outcomes, receipt) = pipeline.execute(wo).await;

    // All stages should succeed.
    for outcome in &outcomes {
        assert!(
            outcome.success,
            "stage {} failed: {:?}",
            outcome.name, outcome.error
        );
    }
    let receipt = receipt.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}
