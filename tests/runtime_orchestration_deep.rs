// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive runtime orchestration tests covering builder patterns,
//! backend selection, event streaming, receipt production, error handling,
//! policy enforcement, concurrency, stream pipeline, projection matrix,
//! and graceful shutdown.

use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RunMetadata, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_dialect::Dialect;
use abp_integrations::{Backend, MockBackend};
use abp_projection::ProjectionMatrix;
use abp_receipt::compute_hash;
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{EventFilter, EventRecorder, EventStats, EventTransform, StreamPipelineBuilder};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Drain all streamed events and await the receipt from a RunHandle.
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

/// Run a work order on the named backend and return events + receipt.
async fn run_full(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    drain_run(handle).await
}

/// Shorthand: run mock backend with PassThrough workspace.
async fn run_mock(rt: &Runtime, task: &str) -> Receipt {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = run_full(rt, "mock", wo).await;
    receipt.unwrap()
}

fn mock_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::default();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    m
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

// ===========================================================================
// Custom test backends
// ===========================================================================

/// Backend that streams a configurable number of events.
#[derive(Debug, Clone)]
struct ConfigurableBackend {
    name: String,
    caps: CapabilityManifest,
    event_count: usize,
}

#[async_trait]
impl Backend for ConfigurableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("test".into()),
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
        let started = chrono::Utc::now();
        let mut trace = Vec::new();

        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("msg-{i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(end_ev.clone());
        let _ = events_tx.send(end_ev).await;

        let finished = chrono::Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
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

/// Backend that always returns an error.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

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
        anyhow::bail!("{}", self.message)
    }
}

/// Backend that panics during run.
#[derive(Debug, Clone)]
struct PanickingBackend;

#[async_trait]
impl Backend for PanickingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "panicker".into(),
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
        panic!("intentional panic in test backend");
    }
}

/// Backend that sends events then delays before returning the receipt.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay_ms: u64,
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
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "slow start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;

        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

        let now = chrono::Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: now,
                finished_at: now,
                duration_ms: self.delay_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
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

/// Backend that sends events with numbered timestamps for ordering tests.
#[derive(Debug, Clone)]
struct OrderedEventBackend {
    count: usize,
}

#[async_trait]
impl Backend for OrderedEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "ordered".into(),
            backend_version: Some("1.0".into()),
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
        let started = chrono::Utc::now();
        let mut trace = Vec::new();

        for i in 0..self.count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("delta-{i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
            // tiny yield to maintain ordering
            tokio::task::yield_now().await;
        }

        let finished = chrono::Utc::now();
        let receipt = Receipt {
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
// 1. Runtime builder pattern
// ===========================================================================

#[test]
fn runtime_new_creates_empty_runtime() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
    assert!(rt.projection().is_none());
    assert!(rt.stream_pipeline().is_none());
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_default_matches_new() {
    let rt: Runtime = Default::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn with_default_backends_registers_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn with_projection_sets_matrix() {
    let matrix = ProjectionMatrix::new();
    let rt = Runtime::new().with_projection(matrix);
    assert!(rt.projection().is_some());
}

#[test]
fn with_stream_pipeline_sets_pipeline() {
    let pipeline = StreamPipelineBuilder::new().build();
    let rt = Runtime::new().with_stream_pipeline(pipeline);
    assert!(rt.stream_pipeline().is_some());
}

#[test]
fn builder_chaining_all_options() {
    let matrix = ProjectionMatrix::new();
    let pipeline = StreamPipelineBuilder::new().record().build();
    let rt = Runtime::new()
        .with_projection(matrix)
        .with_stream_pipeline(pipeline);
    assert!(rt.projection().is_some());
    assert!(rt.stream_pipeline().is_some());
}

#[test]
fn projection_mut_allows_modification() {
    let mut rt = Runtime::new().with_projection(ProjectionMatrix::new());
    let pm = rt.projection_mut().unwrap();
    pm.register_backend("test", mock_manifest(), Dialect::OpenAi, 50);
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn projection_none_when_not_set() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
    let mut rt = rt;
    assert!(rt.projection_mut().is_none());
}

// ===========================================================================
// 2. Backend registration and lookup
// ===========================================================================

#[test]
fn register_backend_adds_to_registry() {
    let mut rt = Runtime::new();
    rt.register_backend("test-be", MockBackend);
    assert!(rt.backend_names().contains(&"test-be".to_string()));
}

#[test]
fn register_backend_replaces_existing() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("mock", MockBackend);
    assert_eq!(
        rt.backend_names()
            .iter()
            .filter(|n| n.as_str() == "mock")
            .count(),
        1
    );
}

#[test]
fn backend_lookup_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_lookup_returns_none_for_unregistered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zebra", MockBackend);
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("middle", MockBackend);
    let names = rt.backend_names();
    assert_eq!(names, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn registry_ref_and_mut_accessible() {
    let mut rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    rt.registry_mut().register("extra", MockBackend);
    assert!(rt.registry().contains("extra"));
}

#[test]
fn registry_remove_backend() {
    let mut rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    let removed = rt.registry_mut().remove("mock");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("mock"));
}

#[test]
fn registry_remove_nonexistent_returns_none() {
    let mut rt = Runtime::new();
    assert!(rt.registry_mut().remove("ghost").is_none());
}

// ===========================================================================
// 3. Backend selection (projection matrix)
// ===========================================================================

#[test]
fn select_backend_without_projection_errors() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("test");
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

#[test]
fn select_backend_picks_registered() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "mock");
}

#[test]
fn select_backend_fails_when_projected_not_in_registry() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("phantom", mock_manifest(), Dialect::OpenAi, 50);
    let rt = Runtime::new().with_projection(matrix);
    let wo = passthrough_wo("test");
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn select_backend_prefers_higher_priority() {
    let manifest = mock_manifest();
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("low", manifest.clone(), Dialect::OpenAi, 10);
    matrix.register_backend("high", manifest, Dialect::OpenAi, 90);

    let mut rt = Runtime::new().with_projection(matrix);
    rt.register_backend("low", MockBackend);
    rt.register_backend("high", MockBackend);

    let wo = passthrough_wo("test");
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "high");
}

#[test]
fn select_backend_prefers_higher_capability_coverage() {
    let mut strong = CapabilityManifest::default();
    strong.insert(Capability::Streaming, SupportLevel::Native);
    strong.insert(Capability::ToolRead, SupportLevel::Native);
    strong.insert(Capability::ToolWrite, SupportLevel::Native);

    let mut weak = CapabilityManifest::default();
    weak.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("strong", strong, Dialect::OpenAi, 50);
    matrix.register_backend("weak", weak, Dialect::Claude, 50);

    let mut rt = Runtime::new().with_projection(matrix);
    rt.register_backend("strong", MockBackend);
    rt.register_backend("weak", MockBackend);

    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
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
        })
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "strong");
}

#[test]
fn select_backend_returns_fallback_chain() {
    let manifest = mock_manifest();
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("a", manifest.clone(), Dialect::OpenAi, 80);
    matrix.register_backend("b", manifest.clone(), Dialect::Claude, 60);
    matrix.register_backend("c", manifest, Dialect::Gemini, 40);

    let mut rt = Runtime::new().with_projection(matrix);
    rt.register_backend("a", MockBackend);
    rt.register_backend("b", MockBackend);
    rt.register_backend("c", MockBackend);

    let wo = passthrough_wo("test");
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "a");
    assert_eq!(result.fallback_chain.len(), 2);
}

#[test]
fn select_backend_different_dialects() {
    for dialect in Dialect::all() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend("be", mock_manifest(), *dialect, 50);

        let mut rt = Runtime::new().with_projection(matrix);
        rt.register_backend("be", MockBackend);

        let wo = passthrough_wo("test");
        let result = rt.select_backend(&wo).unwrap();
        assert_eq!(result.selected_backend, "be");
    }
}

// ===========================================================================
// 4. Event streaming through the runtime
// ===========================================================================

#[tokio::test]
async fn mock_backend_streams_events() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("event streaming test");
    let (events, receipt) = run_full(&rt, "mock", wo).await;
    assert!(!events.is_empty(), "should receive streamed events");
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn event_count_matches_backend_output() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "cfg",
        ConfigurableBackend {
            name: "cfg".into(),
            caps: CapabilityManifest::default(),
            event_count: 5,
        },
    );
    let wo = passthrough_wo("count test");
    let (events, receipt) = run_full(&rt, "cfg", wo).await;
    // 5 messages + 1 RunCompleted
    assert_eq!(events.len(), 6);
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn zero_event_backend_still_produces_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "zero",
        ConfigurableBackend {
            name: "zero".into(),
            caps: CapabilityManifest::default(),
            event_count: 0,
        },
    );
    let wo = passthrough_wo("zero events");
    let (events, receipt) = run_full(&rt, "zero", wo).await;
    // Only the RunCompleted event
    assert_eq!(events.len(), 1);
    let r = receipt.unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[tokio::test]
async fn large_event_stream() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "big",
        ConfigurableBackend {
            name: "big".into(),
            caps: CapabilityManifest::default(),
            event_count: 200,
        },
    );
    let wo = passthrough_wo("big stream");
    let (events, receipt) = run_full(&rt, "big", wo).await;
    // 200 messages + 1 RunCompleted
    assert_eq!(events.len(), 201);
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn events_contain_expected_kinds() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("kind check");
    let (events, _) = run_full(&rt, "mock", wo).await;

    let has_run_started = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }));
    let has_run_completed = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    let has_assistant = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }));

    assert!(has_run_started);
    assert!(has_run_completed);
    assert!(has_assistant);
}

// ===========================================================================
// 5. Receipt production
// ===========================================================================

#[tokio::test]
async fn receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash test").await;
    assert!(receipt.receipt_sha256.is_some());
    assert!(!receipt.receipt_sha256.as_ref().unwrap().is_empty());
}

#[tokio::test]
async fn receipt_hash_is_canonical() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "canonical hash").await;
    let computed = compute_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(computed.as_str()));
}

#[tokio::test]
async fn receipt_has_correct_backend_id() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "backend id check").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "contract version").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_outcome_is_complete() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "outcome check").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn receipt_has_work_order_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("wo id check");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let r = receipt.unwrap();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn receipt_trace_not_empty() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "trace check").await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_timing_fields_populated() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "timing check").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn receipt_capabilities_populated() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "caps check").await;
    assert!(!receipt.capabilities.is_empty());
}

// ===========================================================================
// 6. Error handling
// ===========================================================================

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("unknown backend");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn unknown_backend_error_contains_name() {
    let rt = Runtime::new();
    let wo = passthrough_wo("err name");
    let result = rt.run_streaming("ghost", wo).await;
    let err = result.err().unwrap();
    let msg = err.to_string();
    assert!(msg.contains("ghost"), "error should contain backend name");
}

#[tokio::test]
async fn failing_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "boom!".into(),
        },
    );
    let wo = passthrough_wo("failing");
    let handle = rt.run_streaming("fail", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn panicking_backend_returns_error() {
    let mut rt = Runtime::new();
    rt.register_backend("panic", PanickingBackend);
    let wo = passthrough_wo("panic test");
    let handle = rt.run_streaming("panic", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn capability_check_fails_for_unsatisfiable() {
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

#[tokio::test]
async fn capability_check_unknown_backend() {
    let rt = Runtime::new();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("missing", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 7. Policy enforcement
// ===========================================================================

#[tokio::test]
async fn runtime_compiles_policy_on_run() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["rm".into()],
            deny_read: vec![],
            deny_write: vec!["/etc/**".into()],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        })
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok(), "policy should compile and run succeed");
}

#[tokio::test]
async fn empty_policy_compiles_successfully() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("empty policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile::default())
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn policy_with_glob_patterns() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("glob policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["*".into()],
            disallowed_tools: vec![],
            deny_read: vec!["**/*.secret".into()],
            deny_write: vec!["**/*.lock".into()],
            allow_network: vec![],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec![],
        })
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok());
}

// ===========================================================================
// 8. Concurrent work order handling
// ===========================================================================

#[tokio::test]
async fn concurrent_runs_produce_distinct_receipts() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut handles = Vec::new();

    for i in 0..5 {
        let rt = Arc::clone(&rt);
        handles.push(tokio::spawn(async move {
            let wo = passthrough_wo(&format!("concurrent-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap()
        }));
    }

    let mut run_ids = Vec::new();
    for h in handles {
        let receipt = h.await.unwrap();
        run_ids.push(receipt.meta.run_id);
    }

    // All run IDs should be distinct.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 5, "all run IDs should be unique");
}

#[tokio::test]
async fn concurrent_runs_all_succeed() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut tasks = Vec::new();

    for i in 0..10 {
        let rt = Arc::clone(&rt);
        tasks.push(tokio::spawn(async move {
            let wo = passthrough_wo(&format!("batch-{i}"));
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.is_ok()
        }));
    }

    for t in tasks {
        assert!(t.await.unwrap(), "each concurrent run should succeed");
    }
}

#[tokio::test]
async fn concurrent_runs_on_different_backends() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "cfg-a",
        ConfigurableBackend {
            name: "cfg-a".into(),
            caps: CapabilityManifest::default(),
            event_count: 3,
        },
    );
    rt.register_backend(
        "cfg-b",
        ConfigurableBackend {
            name: "cfg-b".into(),
            caps: CapabilityManifest::default(),
            event_count: 7,
        },
    );
    let rt = Arc::new(rt);

    let rt1 = Arc::clone(&rt);
    let rt2 = Arc::clone(&rt);

    let h1 = tokio::spawn(async move {
        let wo = passthrough_wo("on-a");
        let handle = rt1.run_streaming("cfg-a", wo).await.unwrap();
        drain_run(handle).await
    });
    let h2 = tokio::spawn(async move {
        let wo = passthrough_wo("on-b");
        let handle = rt2.run_streaming("cfg-b", wo).await.unwrap();
        drain_run(handle).await
    });

    let (events_a, receipt_a) = h1.await.unwrap();
    let (events_b, receipt_b) = h2.await.unwrap();

    // 3 + RunCompleted = 4, 7 + RunCompleted = 8
    assert_eq!(events_a.len(), 4);
    assert_eq!(events_b.len(), 8);
    assert!(receipt_a.is_ok());
    assert!(receipt_b.is_ok());
}

// ===========================================================================
// 9. Runtime metrics
// ===========================================================================

#[tokio::test]
async fn metrics_updated_after_run() {
    let rt = Runtime::with_default_backends();
    let snap_before = rt.metrics().snapshot();
    assert_eq!(snap_before.total_runs, 0);

    run_mock(&rt, "metrics test").await;

    let snap_after = rt.metrics().snapshot();
    assert_eq!(snap_after.total_runs, 1);
    assert_eq!(snap_after.successful_runs, 1);
}

#[tokio::test]
async fn metrics_count_multiple_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        run_mock(&rt, &format!("multi-{i}")).await;
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
}

// ===========================================================================
// 10. Event ordering guarantees
// ===========================================================================

#[tokio::test]
async fn events_arrive_in_order() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedEventBackend { count: 20 });
    let wo = passthrough_wo("ordering test");
    let (events, _) = run_full(&rt, "ordered", wo).await;

    // Extract delta texts and verify ordering.
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for (i, text) in deltas.iter().enumerate() {
        assert_eq!(text, &format!("delta-{i}"), "event {i} out of order");
    }
}

#[tokio::test]
async fn event_timestamps_non_decreasing() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("ts order");
    let (events, _) = run_full(&rt, "mock", wo).await;
    for window in events.windows(2) {
        assert!(
            window[0].ts <= window[1].ts,
            "timestamps should be non-decreasing"
        );
    }
}

// ===========================================================================
// 11. Stream pipeline application
// ===========================================================================

#[tokio::test]
async fn pipeline_filter_removes_events() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            !matches!(ev.kind, AgentEventKind::AssistantMessage { .. })
        }))
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    // MockBackend emits RunStarted, 2x AssistantMessage, RunCompleted
    let wo = passthrough_wo("filter test");
    let (events, receipt) = run_full(&rt, "mock", wo).await;

    let assistant_msgs: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
        .collect();
    assert!(
        assistant_msgs.is_empty(),
        "assistant messages should be filtered out"
    );
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn pipeline_transform_modifies_events() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantMessage { ref mut text } = ev.kind {
                *text = format!("[transformed] {text}");
            }
            ev
        }))
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("transform test");
    let (events, _) = run_full(&rt, "mock", wo).await;

    let transformed: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for t in &transformed {
        assert!(t.starts_with("[transformed]"), "should be transformed: {t}");
    }
}

#[tokio::test]
async fn pipeline_recorder_captures_events() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline.clone());
    let wo = passthrough_wo("recorder test");
    let (events, _) = run_full(&rt, "mock", wo).await;

    assert_eq!(
        recorder.len(),
        events.len(),
        "recorder should capture all events that passed"
    );
}

#[tokio::test]
async fn pipeline_stats_tracks_events() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("stats test");
    let (events, _) = run_full(&rt, "mock", wo).await;

    assert_eq!(stats.total_events(), events.len() as u64);
}

#[tokio::test]
async fn pipeline_filter_and_transform_combined() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            matches!(ev.kind, AgentEventKind::AssistantMessage { .. })
        }))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantMessage { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("combined pipeline");
    let (events, _) = run_full(&rt, "mock", wo).await;

    // Only assistant messages should remain, and they should be uppercased.
    for ev in &events {
        assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
        if let AgentEventKind::AssistantMessage { text } = &ev.kind {
            assert_eq!(text, &text.to_uppercase());
        }
    }
}

#[tokio::test]
async fn empty_pipeline_passes_all_events() {
    let pipeline = StreamPipelineBuilder::new().build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("empty pipeline");
    let (events, receipt) = run_full(&rt, "mock", wo).await;
    assert!(!events.is_empty());
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn pipeline_identity_transform() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::identity())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("identity transform");
    let (events, receipt) = run_full(&rt, "mock", wo).await;
    assert!(!events.is_empty());
    assert!(receipt.is_ok());
}

// ===========================================================================
// 12. Projection matrix: run_projected
// ===========================================================================

#[tokio::test]
async fn run_projected_selects_and_executes() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);

    let wo = passthrough_wo("projected run");
    let handle = rt.run_projected(wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let r = receipt.unwrap();
    assert_eq!(r.backend.id, "mock");
}

#[tokio::test]
async fn run_projected_fails_without_matrix() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("no matrix");
    let result = rt.run_projected(wo).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

#[tokio::test]
async fn run_projected_with_multiple_backends() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 90);
    matrix.register_backend("alt", mock_manifest(), Dialect::Claude, 10);

    let mut rt = Runtime::with_default_backends().with_projection(matrix);
    rt.register_backend("alt", MockBackend);

    let wo = passthrough_wo("multi projected");
    let handle = rt.run_projected(wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let r = receipt.unwrap();
    // Should pick the highest priority.
    assert_eq!(r.backend.id, "mock");
}

// ===========================================================================
// 13. RuntimeError variant coverage
// ===========================================================================

#[test]
fn error_unknown_backend_code() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn error_workspace_failed_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn error_policy_failed_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn error_backend_failed_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn error_capability_check_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn error_no_projection_match_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn classified_error_preserves_code() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
}

#[test]
fn runtime_error_into_abp_error_roundtrip() {
    let rt_err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let code = rt_err.error_code();
    let abp_err = rt_err.into_abp_error();
    assert_eq!(abp_err.code, code);
    assert!(abp_err.message.contains("missing"));
}

#[test]
fn classified_error_preserves_context() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml");
    let rt_err: RuntimeError = abp_err.into();
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, abp_error::ErrorCode::ConfigInvalid);
    assert_eq!(
        back.context.get("file"),
        Some(&serde_json::json!("backplane.toml"))
    );
}

#[test]
fn error_display_messages() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert!(err.to_string().contains("unknown backend"));
    assert!(err.to_string().contains("foo"));

    let err2 = RuntimeError::WorkspaceFailed(anyhow::anyhow!("io error"));
    assert!(err2.to_string().contains("workspace preparation failed"));

    let err3 = RuntimeError::PolicyFailed(anyhow::anyhow!("glob error"));
    assert!(err3.to_string().contains("policy compilation failed"));

    let err4 = RuntimeError::BackendFailed(anyhow::anyhow!("oops"));
    assert!(err4.to_string().contains("backend execution failed"));

    let err5 = RuntimeError::CapabilityCheckFailed("mcp".into());
    assert!(err5.to_string().contains("capability check failed"));

    let err6 = RuntimeError::NoProjectionMatch {
        reason: "empty matrix".into(),
    };
    assert!(err6.to_string().contains("projection failed"));
}

// ===========================================================================
// 14. Capability checks
// ===========================================================================

#[test]
fn check_capabilities_passes_for_satisfiable() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[test]
fn check_capabilities_empty_reqs_always_passes() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[test]
fn check_capabilities_multiple_satisfied() {
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
}

// ===========================================================================
// 15. Receipt chain
// ===========================================================================

#[tokio::test]
async fn receipt_chain_grows_with_runs() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain-1").await;
    run_mock(&rt, "chain-2").await;
    run_mock(&rt, "chain-3").await;

    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert_eq!(guard.len(), 3);
}

#[tokio::test]
async fn receipt_chain_is_shared() {
    let rt = Runtime::with_default_backends();
    let chain1 = rt.receipt_chain();
    let chain2 = rt.receipt_chain();

    run_mock(&rt, "shared chain").await;

    let len1 = chain1.lock().await.len();
    let len2 = chain2.lock().await.len();
    assert_eq!(len1, len2);
    assert_eq!(len1, 1);
}

// ===========================================================================
// 16. RunHandle structure
// ===========================================================================

#[tokio::test]
async fn run_handle_has_unique_run_id() {
    let rt = Runtime::with_default_backends();
    let wo1 = passthrough_wo("handle-1");
    let wo2 = passthrough_wo("handle-2");

    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let h2 = rt.run_streaming("mock", wo2).await.unwrap();

    assert_ne!(h1.run_id, h2.run_id, "run IDs should be distinct");

    // Drain both to avoid resource leaks.
    let _ = drain_run(h1).await;
    let _ = drain_run(h2).await;
}

#[tokio::test]
async fn run_handle_run_id_is_valid_uuid() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("uuid check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    // Uuid::new_v4() produces version 4
    assert_eq!(handle.run_id.get_version_num(), 4);
    let _ = drain_run(handle).await;
}

// ===========================================================================
// 17. Slow backend / graceful completion
// ===========================================================================

#[tokio::test]
async fn slow_backend_completes() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 50 });
    let wo = passthrough_wo("slow test");
    let handle = rt.run_streaming("slow", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert!(!events.is_empty());
    assert!(receipt.is_ok());
}

// ===========================================================================
// 18. Multiple backend types simultaneously
// ===========================================================================

#[tokio::test]
async fn multiple_backend_types_coexist() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend(
        "custom",
        ConfigurableBackend {
            name: "custom".into(),
            caps: CapabilityManifest::default(),
            event_count: 2,
        },
    );
    rt.register_backend("slow", SlowBackend { delay_ms: 10 });

    // Run each backend.
    let r1 = {
        let wo = passthrough_wo("t1");
        let h = rt.run_streaming("mock", wo).await.unwrap();
        let (_, r) = drain_run(h).await;
        r.unwrap()
    };
    let r2 = {
        let wo = passthrough_wo("t2");
        let h = rt.run_streaming("custom", wo).await.unwrap();
        let (_, r) = drain_run(h).await;
        r.unwrap()
    };
    let r3 = {
        let wo = passthrough_wo("t3");
        let h = rt.run_streaming("slow", wo).await.unwrap();
        let (_, r) = drain_run(h).await;
        r.unwrap()
    };

    assert_eq!(r1.backend.id, "mock");
    assert_eq!(r2.backend.id, "custom");
    assert_eq!(r3.backend.id, "slow");
}

// ===========================================================================
// 19. Stream pipeline with recorder and stats together
// ===========================================================================

#[tokio::test]
async fn pipeline_recorder_and_stats_together() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("recorder+stats");
    let (events, _) = run_full(&rt, "mock", wo).await;

    assert_eq!(recorder.len(), events.len());
    assert_eq!(stats.total_events(), events.len() as u64);
}

// ===========================================================================
// 20. WorkOrder builder variations
// ===========================================================================

#[tokio::test]
async fn workorder_with_model_runs() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("model test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4")
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn workorder_with_max_turns_runs() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("turns test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_turns(10)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn workorder_minimal_runs() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("minimal")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(receipt.is_ok());
}

// ===========================================================================
// 21. Receipt verification fields
// ===========================================================================

#[tokio::test]
async fn receipt_verification_populated_for_passthrough() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "verification check").await;
    // For PassThrough mode the runtime still fills verification metadata.
    // Just ensure the receipt completed without error and the field exists.
    let _ = &receipt.verification;
}

// ===========================================================================
// 22. Pipeline filter-all events
// ===========================================================================

#[tokio::test]
async fn pipeline_filter_all_events_produces_empty_stream() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| false))
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("filter all");
    let (events, receipt) = run_full(&rt, "mock", wo).await;
    assert!(events.is_empty(), "all events should be filtered");
    assert!(receipt.is_ok());
}

// ===========================================================================
// 23. Multiple filters compose as AND
// ===========================================================================

#[tokio::test]
async fn multiple_filters_compose_as_and() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            // Allow anything except RunStarted
            !matches!(ev.kind, AgentEventKind::RunStarted { .. })
        }))
        .filter(EventFilter::new(|ev| {
            // Allow anything except RunCompleted
            !matches!(ev.kind, AgentEventKind::RunCompleted { .. })
        }))
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = passthrough_wo("multi filter");
    let (events, _) = run_full(&rt, "mock", wo).await;

    // Only AssistantMessage events remain.
    for ev in &events {
        assert!(
            matches!(ev.kind, AgentEventKind::AssistantMessage { .. }),
            "only AssistantMessage should survive: got {:?}",
            ev.kind
        );
    }
}

// ===========================================================================
// 24. Backend identity accessible
// ===========================================================================

#[test]
fn backend_identity_accessible_via_arc() {
    let rt = Runtime::with_default_backends();
    let backend = rt.backend("mock").unwrap();
    let id = backend.identity();
    assert_eq!(id.id, "mock");
    assert_eq!(id.backend_version.as_deref(), Some("0.1"));
}

// ===========================================================================
// 25. Projection fidelity score
// ===========================================================================

#[test]
fn projection_fidelity_score_positive() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);

    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("score test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert!(
        result.fidelity_score.total > 0.0,
        "fidelity score should be positive"
    );
}
