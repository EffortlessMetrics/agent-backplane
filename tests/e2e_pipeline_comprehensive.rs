#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
//! Comprehensive end-to-end pipeline tests: work order → backend → receipt.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use abp_config::{
    load_config, load_from_str, merge_configs, parse_toml, validate_config, BackendEntry,
    BackplaneConfig, ConfigError, ConfigWarning,
};
use abp_core::{
    canonical_json, receipt_hash, sha256_hex, AgentEvent, AgentEventKind, ArtifactRef,
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
use abp_receipt::{compute_hash, verify_hash, ReceiptChain};
use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker, BudgetViolation};
use abp_runtime::bus::{EventBus, EventBusStats, FilteredSubscription};
use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};
use abp_runtime::pipeline::{AuditStage, Pipeline, PolicyStage, ValidationStage};
use abp_runtime::store::ReceiptStore;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use abp_stream::{EventFilter, EventRecorder, EventStats, StreamPipeline, StreamPipelineBuilder};
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

async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

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
struct CountingBackend {
    count: usize,
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".into(),
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
        let mut trace = Vec::new();
        let ev = make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        });
        trace.push(ev.clone());
        let _ = tx.send(ev).await;
        for i in 0..self.count {
            let ev = make_event(AgentEventKind::AssistantMessage {
                text: format!("msg-{i}"),
            });
            trace.push(ev.clone());
            let _ = tx.send(ev).await;
        }
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
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
            usage_raw: json!({}),
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

#[derive(Debug, Clone)]
struct ToolUseBackend;

#[async_trait]
impl Backend for ToolUseBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-use".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::ToolRead, abp_core::SupportLevel::Native);
        m.insert(Capability::ToolWrite, abp_core::SupportLevel::Native);
        m.insert(Capability::Streaming, abp_core::SupportLevel::Native);
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
        for ev in [
            make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            }),
            make_event(AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("t1".into()),
                output: json!("fn main() {}"),
                is_error: false,
            }),
            make_event(AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added logging".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ] {
            trace.push(ev.clone());
            let _ = tx.send(ev).await;
        }
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
            usage: UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50),
                ..Default::default()
            },
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
// 1. Full pipeline: work order → mock backend → receipt (10 tests)
// ===========================================================================

#[tokio::test]
async fn pipeline_mock_basic_complete() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "basic test").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn pipeline_mock_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "hash test").await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn pipeline_mock_receipt_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "version test").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn pipeline_mock_receipt_backend_identity() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "identity test").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn pipeline_mock_receipt_timing() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "timing test").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn pipeline_mock_events_contain_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "started event").await;
    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    assert!(has_started);
}

#[tokio::test]
async fn pipeline_mock_events_contain_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "completed event").await;
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_completed);
}

#[tokio::test]
async fn pipeline_mock_events_contain_assistant_message() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "assistant msg").await;
    let has_msg = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    assert!(has_msg);
}

#[tokio::test]
async fn pipeline_mock_receipt_trace_nonempty() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "trace test").await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn pipeline_mock_work_order_id_in_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("id test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

// ===========================================================================
// 2. Receipt hashing and verification (10 tests)
// ===========================================================================

#[test]
fn receipt_hash_is_deterministic() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_hash_populates_sha256() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_length_is_64() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_hash_ignores_stored_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("garbage".into());
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn verify_hash_returns_true_for_valid() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_returns_true_when_no_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_returns_false_for_tampered() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn receipt_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn receipt_hash_differs_for_different_backends() {
    let r1 = ReceiptBuilder::new("mock-a")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock-b")
        .outcome(Outcome::Complete)
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn compute_hash_matches_receipt_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt_hash(&r).unwrap(), compute_hash(&r).unwrap());
}

// ===========================================================================
// 3. Receipt chain (8 tests)
// ===========================================================================

#[test]
fn receipt_chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn receipt_chain_push_single() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn receipt_chain_push_multiple() {
    let mut chain = ReceiptChain::new();
    for _ in 0..5 {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
}

#[tokio::test]
async fn runtime_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain-1").await;
    run_mock(&rt, "chain-2").await;
    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert!(locked.len() >= 2);
}

#[test]
fn receipt_chain_entries_have_hashes() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    chain.push(r.clone()).unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_chain_preserves_order() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        let r = ReceiptBuilder::new(format!("b-{i}"))
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.get(0).unwrap().backend.id, "b-0");
    assert_eq!(chain.get(1).unwrap().backend.id, "b-1");
    assert_eq!(chain.get(2).unwrap().backend.id, "b-2");
}

#[test]
fn receipt_builder_sets_outcome() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_sets_backend_version() {
    let r = ReceiptBuilder::new("mock").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

// ===========================================================================
// 4. Error propagation through pipeline (10 tests)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let err = rt
        .run_streaming("nonexistent", wo)
        .await
        .err()
        .expect("should fail");
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn error_failing_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("fail", FailingBackend);
    let wo = WorkOrderBuilder::new("fail test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("fail", wo).await.unwrap();
    let (_, result) = drain_run(handle).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::BackendFailed(_)
    ));
}

#[tokio::test]
async fn error_capability_check_fails_for_unsatisfied() {
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
async fn error_capability_check_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("no-such-backend", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn runtime_error_has_error_code() {
    let e = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(e.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_backend_failed_is_retryable() {
    let e = RuntimeError::BackendFailed(anyhow::anyhow!("boom"));
    assert!(e.is_retryable());
}

#[test]
fn runtime_error_unknown_backend_not_retryable() {
    let e = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(!e.is_retryable());
}

#[test]
fn runtime_error_capability_not_retryable() {
    let e = RuntimeError::CapabilityCheckFailed("missing".into());
    assert!(!e.is_retryable());
}

#[test]
fn runtime_error_into_abp_error() {
    let e = RuntimeError::UnknownBackend { name: "z".into() };
    let abp = e.into_abp_error();
    assert_eq!(abp.code, abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_display() {
    let e = RuntimeError::UnknownBackend {
        name: "test-be".into(),
    };
    assert!(e.to_string().contains("test-be"));
}

// ===========================================================================
// 5. Config loading → backend selection (10 tests)
// ===========================================================================

#[test]
fn config_default_is_valid() {
    let cfg = BackplaneConfig::default();
    validate_config(&cfg).expect("default valid");
}

#[test]
fn config_parse_mock_backend() {
    let toml = r#"
        default_backend = "mock"
        [backends.mock]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert!(cfg.backends.contains_key("mock"));
}

#[test]
fn config_parse_sidecar_backend() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        timeout_secs = 120
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args.len(), 1);
            assert_eq!(*timeout_secs, Some(120));
        }
        _ => panic!("expected sidecar"),
    }
}

#[test]
fn config_validation_bad_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn config_validation_empty_sidecar_cmd() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: " ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn config_merge_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

#[test]
fn config_merge_combines_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn config_load_none_returns_default() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn config_invalid_toml_gives_parse_error() {
    let err = parse_toml("[bad = ").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn config_empty_string_parses_ok() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.backends.is_empty());
}

// ===========================================================================
// 6. Event streaming verification (10 tests)
// ===========================================================================

#[tokio::test]
async fn event_ordering_run_started_first() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "order test").await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn event_ordering_run_completed_last() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "order test").await;
    let last = events.last().unwrap();
    assert!(matches!(&last.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn event_timestamps_monotonic() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "ts test").await;
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[tokio::test]
async fn custom_backend_emits_correct_count() {
    let mut rt = Runtime::new();
    rt.register_backend("cnt", CountingBackend { count: 5 });
    let wo = WorkOrderBuilder::new("count test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("cnt", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // 1 start + 5 messages + 1 completed = 7
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 5);
}

#[tokio::test]
async fn tool_use_backend_emits_tool_events() {
    let mut rt = Runtime::new();
    rt.register_backend("tool", ToolUseBackend);
    let wo = WorkOrderBuilder::new("tool test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("tool", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let has_tool_call = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    let has_tool_result = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }));
    assert!(has_tool_call);
    assert!(has_tool_result);
}

#[tokio::test]
async fn tool_use_backend_emits_file_changed() {
    let mut rt = Runtime::new();
    rt.register_backend("tool", ToolUseBackend);
    let wo = WorkOrderBuilder::new("file changed test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("tool", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let has_fc = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. }));
    assert!(has_fc);
}

#[tokio::test]
async fn event_ext_is_none_for_mock() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "ext test").await;
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn events_match_receipt_trace() {
    let mut rt = Runtime::new();
    rt.register_backend("cnt", CountingBackend { count: 3 });
    let wo = WorkOrderBuilder::new("trace match")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("cnt", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Events and trace should have the same count
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn stream_pipeline_records_events() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "pipeline recorder").await;
    assert!(recorder.len() > 0);
}

#[tokio::test]
async fn stream_pipeline_stats_count() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "stats test").await;
    assert!(stats.total_events() > 0);
}

// ===========================================================================
// 7. Multiple backend scenarios (8 tests)
// ===========================================================================

#[tokio::test]
async fn register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("cnt", CountingBackend { count: 1 });
    rt.register_backend("tool", ToolUseBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
    assert!(names.contains(&"cnt".to_string()));
    assert!(names.contains(&"tool".to_string()));
}

#[tokio::test]
async fn run_against_different_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("cnt", CountingBackend { count: 2 });
    let (_, r1) = run_mock(&rt, "mock run").await;
    let wo = WorkOrderBuilder::new("cnt run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("cnt", wo).await.unwrap();
    let (_, r2) = drain_run(handle).await;
    let r2 = r2.unwrap();
    assert_eq!(r1.backend.id, "mock");
    assert_eq!(r2.backend.id, "counting");
}

#[tokio::test]
async fn backend_replacement() {
    let mut rt = Runtime::new();
    rt.register_backend("test", CountingBackend { count: 1 });
    rt.register_backend("test", CountingBackend { count: 5 });
    let wo = WorkOrderBuilder::new("replacement test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 5);
}

#[test]
fn backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zulu", MockBackend);
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("mike", MockBackend);
    let names = rt.backend_names();
    assert_eq!(names, vec!["alpha", "mike", "zulu"]);
}

#[test]
fn backend_lookup_existing() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_lookup_missing() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_registry_contains() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nope"));
}

#[test]
fn backend_registry_list() {
    let mut rt = Runtime::new();
    rt.register_backend("a", MockBackend);
    rt.register_backend("b", MockBackend);
    let list = rt.registry().list();
    assert_eq!(list.len(), 2);
}

// ===========================================================================
// 8. WorkOrder builder (10 tests)
// ===========================================================================

#[test]
fn work_order_builder_basic() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn work_order_builder_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_builder_root() {
    let wo = WorkOrderBuilder::new("t").root("/tmp/ws").build();
    assert_eq!(wo.workspace.root, "/tmp/ws");
}

#[test]
fn work_order_builder_workspace_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn work_order_builder_model() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_builder_max_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn work_order_builder_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["Read"]);
}

#[test]
fn work_order_builder_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

// ===========================================================================
// 9. Policy enforcement (8 tests)
// ===========================================================================

#[test]
fn policy_engine_empty_allows_all() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
}

#[test]
fn policy_engine_deny_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn policy_engine_allow_tool() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn policy_engine_deny_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(std::path::Path::new("data.secret"));
    assert!(!decision.allowed);
}

#[test]
fn policy_engine_allow_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["*.secret".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(std::path::Path::new("data.txt"));
    assert!(decision.allowed);
}

#[test]
fn policy_engine_deny_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(std::path::Path::new("Cargo.lock"));
    assert!(!decision.allowed);
}

#[tokio::test]
async fn pipeline_validation_rejects_empty_task() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = WorkOrderBuilder::new("").build();
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn pipeline_validation_passes_valid_order() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = WorkOrderBuilder::new("valid task").build();
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_ok());
}

// ===========================================================================
// 10. Budget tracking (6 tests)
// ===========================================================================

#[test]
fn budget_unlimited_always_within() {
    let t = BudgetTracker::new(BudgetLimit::default());
    t.record_tokens(1_000_000);
    t.record_cost(1_000.0);
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

#[test]
fn budget_token_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..Default::default()
    });
    t.record_tokens(101);
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TokensExceeded { .. })
    ));
}

#[test]
fn budget_cost_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_cost_usd: Some(1.0),
        ..Default::default()
    });
    t.record_cost(1.5);
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::CostExceeded { .. })
    ));
}

#[test]
fn budget_turns_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_turns: Some(3),
        ..Default::default()
    });
    t.record_turn();
    t.record_turn();
    t.record_turn();
    t.record_turn();
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded { .. })
    ));
}

#[test]
fn budget_remaining_decreases() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        ..Default::default()
    });
    t.record_tokens(300);
    let rem = t.remaining();
    assert_eq!(rem.tokens, Some(700));
}

#[test]
fn budget_warning_near_limit() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..Default::default()
    });
    t.record_tokens(85);
    assert!(matches!(t.check(), BudgetStatus::Warning { .. }));
}

// ===========================================================================
// 11. Cancellation primitives (6 tests)
// ===========================================================================

#[test]
fn cancellation_token_starts_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel_flips() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancellation_token_clone_shares_state() {
    let a = CancellationToken::new();
    let b = a.clone();
    a.cancel();
    assert!(b.is_cancelled());
}

#[test]
fn cancellable_run_records_reason() {
    let token = CancellationToken::new();
    let run = CancellableRun::new(token);
    run.cancel(CancellationReason::UserRequested);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(CancellationReason::UserRequested));
}

#[test]
fn cancellable_run_first_reason_wins() {
    let token = CancellationToken::new();
    let run = CancellableRun::new(token);
    run.cancel(CancellationReason::Timeout);
    run.cancel(CancellationReason::BudgetExhausted);
    assert_eq!(run.reason(), Some(CancellationReason::Timeout));
}

#[test]
fn cancellation_reason_description() {
    assert!(!CancellationReason::UserRequested.description().is_empty());
    assert!(!CancellationReason::Timeout.description().is_empty());
    assert!(!CancellationReason::BudgetExhausted.description().is_empty());
    assert!(!CancellationReason::PolicyViolation.description().is_empty());
    assert!(!CancellationReason::SystemShutdown.description().is_empty());
}

// ===========================================================================
// 12. Event bus (6 tests)
// ===========================================================================

#[test]
fn event_bus_new_has_zero_stats() {
    let bus = EventBus::new();
    let stats = bus.stats();
    assert_eq!(stats.total_published, 0);
    assert_eq!(stats.dropped_events, 0);
}

#[test]
fn event_bus_publish_increments_count() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    }));
    assert_eq!(bus.stats().total_published, 1);
}

#[test]
fn event_bus_subscriber_count() {
    let bus = EventBus::new();
    let s1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);
    let s2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);
    drop(s1);
    assert_eq!(bus.subscriber_count(), 1);
    drop(s2);
    assert_eq!(bus.subscriber_count(), 0);
}

#[tokio::test]
async fn event_bus_subscriber_receives() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    }));
    let ev = sub.recv().await;
    assert!(ev.is_some());
}

#[test]
fn event_bus_no_subscribers_counts_dropped() {
    let bus = EventBus::new();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "nope".into(),
    }));
    assert_eq!(bus.stats().dropped_events, 1);
}

#[test]
fn event_bus_with_capacity() {
    let bus = EventBus::with_capacity(16);
    let _sub = bus.subscribe();
    for i in 0..10 {
        bus.publish(make_event(AgentEventKind::AssistantMessage {
            text: format!("msg-{i}"),
        }));
    }
    assert_eq!(bus.stats().total_published, 10);
}

// ===========================================================================
// 13. Pipeline stages (6 tests)
// ===========================================================================

#[tokio::test]
async fn pipeline_empty_passes() {
    let pipeline = Pipeline::new();
    let mut wo = WorkOrderBuilder::new("test").build();
    assert!(pipeline.execute(&mut wo).await.is_ok());
}

#[tokio::test]
async fn pipeline_validation_and_audit() {
    let audit = Arc::new(AuditStage::new());
    let pipeline = Pipeline::new()
        .stage(ValidationStage)
        .stage(AuditStage::new());
    let mut wo = WorkOrderBuilder::new("audit me").build();
    pipeline.execute(&mut wo).await.unwrap();
}

#[tokio::test]
async fn pipeline_policy_passes_clean_order() {
    let pipeline = Pipeline::new().stage(PolicyStage);
    let mut wo = WorkOrderBuilder::new("clean order").build();
    assert!(pipeline.execute(&mut wo).await.is_ok());
}

#[test]
fn pipeline_len_and_empty() {
    let p = Pipeline::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    let p = Pipeline::new().stage(ValidationStage).stage(PolicyStage);
    assert!(!p.is_empty());
    assert_eq!(p.len(), 2);
}

#[tokio::test]
async fn audit_stage_records_entries() {
    let audit = AuditStage::new();
    let pipeline = Pipeline::new().stage(ValidationStage);
    // We use the audit stage directly
    let audit2 = AuditStage::new();
    let mut wo = WorkOrderBuilder::new("audit test").build();
    let wo_id = wo.id;
    // Process through audit manually
    use abp_runtime::pipeline::PipelineStage;
    audit2.process(&mut wo).await.unwrap();
    let entries = audit2.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].work_order_id, wo_id);
}

#[tokio::test]
async fn pipeline_short_circuits_on_error() {
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);
    let mut wo = WorkOrderBuilder::new("").build(); // empty task
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
}

// ===========================================================================
// 14. Telemetry and metrics (6 tests)
// ===========================================================================

#[tokio::test]
async fn runtime_metrics_after_run() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "metrics test").await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_runs >= 1);
    assert!(snap.successful_runs >= 1);
}

#[tokio::test]
async fn runtime_metrics_events_counted() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "event count metrics").await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn runtime_metrics_after_multiple_runs() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "run-1").await;
    run_mock(&rt, "run-2").await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_runs >= 2);
}

#[test]
fn telemetry_collector_records() {
    let collector = abp_telemetry::MetricsCollector::new();
    collector.record(abp_telemetry::RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 100,
        events_count: 5,
        tokens_in: 10,
        tokens_out: 20,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    });
    assert_eq!(collector.len(), 1);
}

#[test]
fn telemetry_collector_summary() {
    let collector = abp_telemetry::MetricsCollector::new();
    collector.record(abp_telemetry::RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 100,
        events_count: 5,
        tokens_in: 10,
        tokens_out: 20,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    });
    collector.record(abp_telemetry::RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 200,
        events_count: 3,
        tokens_in: 30,
        tokens_out: 40,
        tool_calls_count: 0,
        errors_count: 1,
        emulations_applied: 0,
    });
    let summary = collector.summary();
    assert_eq!(summary.count, 2);
    assert!(summary.error_rate > 0.0);
}

#[test]
fn telemetry_run_summary_from_events() {
    let summary = abp_telemetry::RunSummary::from_events(
        &["run_started", "assistant_message", "error", "run_completed"],
        100,
    );
    assert!(summary.has_errors());
    assert!(summary.error_rate() > 0.0);
}

// ===========================================================================
// 15. Capability checking (6 tests)
// ===========================================================================

#[test]
fn capability_check_passes_satisfiable() {
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
fn capability_check_passes_emulated() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[test]
fn capability_check_fails_native_for_emulated() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    // MockBackend has ToolRead as Emulated, requesting Native should fail
    assert!(rt.check_capabilities("mock", &reqs).is_err());
}

#[test]
fn capability_check_empty_requirements() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn mock_backend_identity() {
    let backend = MockBackend;
    let id = backend.identity();
    assert_eq!(id.id, "mock");
}

#[test]
fn mock_backend_capabilities_nonempty() {
    let backend = MockBackend;
    let caps = backend.capabilities();
    assert!(!caps.is_empty());
    assert!(caps.contains_key(&Capability::Streaming));
}

// ===========================================================================
// 16. Core contract types (6 tests)
// ===========================================================================

#[test]
fn contract_version_is_correct() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn canonical_json_deterministic() {
    let val = json!({"z": 1, "a": 2, "m": 3});
    let j1 = canonical_json(&val).unwrap();
    let j2 = canonical_json(&val).unwrap();
    assert_eq!(j1, j2);
    assert!(j1.starts_with(r#"{"a":2"#));
}

#[test]
fn sha256_hex_correct_length() {
    let h = sha256_hex(b"hello world");
    assert_eq!(h.len(), 64);
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = sha256_hex(b"test");
    let h2 = sha256_hex(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn outcome_serde_roundtrip() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    for o in &outcomes {
        let json = serde_json::to_string(o).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, o);
    }
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

// ===========================================================================
// 17. Stream pipeline (6 tests)
// ===========================================================================

#[test]
fn stream_pipeline_passthrough() {
    let pipeline = StreamPipeline::new();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    let result = pipeline.process(ev);
    assert!(result.is_some());
}

#[test]
fn stream_pipeline_filter_excludes() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    assert!(pipeline.process(ev).is_none());
}

#[test]
fn stream_pipeline_filter_includes() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    let ev = make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    });
    assert!(pipeline.process(ev).is_some());
}

#[test]
fn stream_pipeline_recorder() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    pipeline.process(make_event(AgentEventKind::RunStarted {
        message: "r".into(),
    }));
    assert_eq!(recorder.len(), 1);
}

#[test]
fn stream_pipeline_stats_tracking() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    pipeline.process(make_event(AgentEventKind::RunStarted {
        message: "s".into(),
    }));
    pipeline.process(make_event(AgentEventKind::AssistantMessage {
        text: "m".into(),
    }));
    assert_eq!(stats.total_events(), 2);
}

#[test]
fn event_filter_by_kind() {
    let filter = EventFilter::by_kind("assistant_message");
    let ev_msg = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let ev_start = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(filter.matches(&ev_msg));
    assert!(!filter.matches(&ev_start));
}

// ===========================================================================
// 18. ReceiptStore (4 tests)
// ===========================================================================

#[test]
fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let run_id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
}

#[test]
fn receipt_store_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let list = store.list().unwrap();
    assert!(list.is_empty());
}

#[test]
fn receipt_store_list_after_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn receipt_store_load_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let result = store.load(Uuid::new_v4());
    assert!(result.is_err());
}

// ===========================================================================
// 19. Serde and serialization (4 tests)
// ===========================================================================

#[test]
fn work_order_serializes_to_json() {
    let wo = WorkOrderBuilder::new("serde test").build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("serde test"));
}

#[test]
fn receipt_serializes_to_json() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("mock"));
}

#[test]
fn agent_event_serializes_roundtrip() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn backend_entry_serde_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["host.js".into()],
        timeout_secs: Some(60),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    match back {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        _ => panic!("expected sidecar"),
    }
}
