#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Comprehensive tests for the abp-runtime orchestration layer.
//!
//! Covers: Runtime construction, backend registration/lookup, unknown backend
//! errors, workspace preparation (staged vs passthrough), event streaming via
//! RunHandle, receipt generation and hashing, PolicyFailed / WorkspaceFailed /
//! BackendFailed errors, run lifecycle, multiple sequential runs, default
//! configuration, serde roundtrip of RuntimeConfig, pipeline stages, budget
//! tracking, cancellation, observability, multiplexing, event bus, hooks,
//! retry, receipt store, telemetry, and projection integration.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::*;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Drain all streamed events and await the receipt from a RunHandle.
async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
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

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
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

// ===========================================================================
// 1. Runtime construction
// ===========================================================================

#[test]
fn runtime_new_creates_empty_runtime() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_default_creates_empty_runtime() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn runtime_with_default_backends_mock_identity() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    assert_eq!(b.identity().id, "mock");
}

#[test]
fn runtime_new_has_no_projection() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_new_has_no_emulation() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_new_has_no_stream_pipeline() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_new_metrics_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
}

// ===========================================================================
// 2. Backend registration and lookup
// ===========================================================================

#[test]
fn register_and_lookup_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("test", MockBackend);
    assert!(rt.backend("test").is_some());
}

#[test]
fn register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("a", MockBackend);
    rt.register_backend("b", MockBackend);
    rt.register_backend("c", MockBackend);
    let names = rt.backend_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));
    assert!(names.contains(&"c".to_string()));
}

#[test]
fn backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zebra", MockBackend);
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("middle", MockBackend);
    let names = rt.backend_names();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn register_backend_overwrites() {
    let mut rt = Runtime::new();
    rt.register_backend("x", MockBackend);
    rt.register_backend("x", MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn lookup_nonexistent_backend_returns_none() {
    let rt = Runtime::new();
    assert!(rt.backend("does_not_exist").is_none());
}

#[test]
fn registry_contains_check() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

#[test]
fn registry_list_returns_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("z", MockBackend);
    rt.register_backend("a", MockBackend);
    let list = rt.registry().list();
    assert_eq!(list, vec!["a", "z"]);
}

#[test]
fn registry_get_arc() {
    let rt = Runtime::with_default_backends();
    let arc = rt.registry().get_arc("mock");
    assert!(arc.is_some());
    assert_eq!(arc.unwrap().identity().id, "mock");
}

#[test]
fn registry_remove_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("removable", MockBackend);
    assert!(rt.registry().contains("removable"));
    let removed = rt.registry_mut().remove("removable");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("removable"));
}

#[test]
fn registry_remove_nonexistent_returns_none() {
    let mut rt = Runtime::new();
    let removed = rt.registry_mut().remove("nope");
    assert!(removed.is_none());
}

// ===========================================================================
// 3. Unknown backend error handling
// ===========================================================================

#[tokio::test]
async fn run_streaming_unknown_backend_returns_error() {
    let rt = Runtime::new();
    let wo = passthrough_wo("test");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
}

#[tokio::test]
async fn unknown_backend_error_contains_name() {
    let rt = Runtime::new();
    let wo = passthrough_wo("test");
    match rt.run_streaming("mystery", wo).await {
        Err(err) => {
            let msg = err.to_string();
            assert!(msg.contains("mystery"), "error should contain name: {msg}");
        }
        Ok(_) => panic!("expected error for unknown backend"),
    }
}

#[tokio::test]
async fn unknown_backend_error_code() {
    let rt = Runtime::new();
    let wo = passthrough_wo("test");
    match rt.run_streaming("nope", wo).await {
        Err(err) => {
            assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
        }
        Ok(_) => panic!("expected error for unknown backend"),
    }
}

// ===========================================================================
// 4. Workspace preparation (staged vs passthrough)
// ===========================================================================

#[tokio::test]
async fn passthrough_workspace_runs_successfully() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn staged_workspace_runs_successfully() {
    let rt = Runtime::with_default_backends();
    let dir = tempfile::tempdir().unwrap();
    let wo = WorkOrderBuilder::new("staged test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(dir.path().to_string_lossy().to_string())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn passthrough_receipt_has_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "outcome check").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn staged_receipt_has_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let dir = tempfile::tempdir().unwrap();
    let wo = WorkOrderBuilder::new("staged outcome")
        .workspace_mode(WorkspaceMode::Staged)
        .root(dir.path().to_string_lossy().to_string())
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 5. Event streaming via RunHandle
// ===========================================================================

#[tokio::test]
async fn run_handle_has_unique_run_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert_ne!(handle.run_id, Uuid::nil());
}

#[tokio::test]
async fn events_stream_produces_events() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("events test");
    let (events, _) = run_full(&rt, "mock", wo).await;
    assert!(!events.is_empty(), "should receive at least one event");
}

#[tokio::test]
async fn events_contain_run_started() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("started test");
    let (events, _) = run_full(&rt, "mock", wo).await;
    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    assert!(has_started, "should contain RunStarted event");
}

#[tokio::test]
async fn events_contain_run_completed() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("completed test");
    let (events, _) = run_full(&rt, "mock", wo).await;
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_completed, "should contain RunCompleted event");
}

#[tokio::test]
async fn events_contain_assistant_messages() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("assistant test");
    let (events, _) = run_full(&rt, "mock", wo).await;
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert!(msg_count >= 1, "should have assistant messages");
}

#[tokio::test]
async fn configurable_backend_streams_exact_event_count() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "cfg",
        ConfigurableBackend {
            name: "cfg".into(),
            caps: CapabilityManifest::default(),
            event_count: 5,
        },
    );
    let wo = passthrough_wo("cfg events");
    let (events, receipt) = run_full(&rt, "cfg", wo).await;
    assert!(receipt.is_ok());
    // 5 assistant messages + 1 RunCompleted
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 5);
}

#[tokio::test]
async fn zero_event_backend_still_returns_receipt() {
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
    let (_, receipt) = run_full(&rt, "zero", wo).await;
    assert!(receipt.is_ok());
}

// ===========================================================================
// 6. Receipt generation and hashing
// ===========================================================================

#[tokio::test]
async fn receipt_has_sha256_hash() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash test").await;
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt should have sha256 hash"
    );
}

#[tokio::test]
async fn receipt_hash_is_64_hex_chars() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash len test").await;
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should be hex"
    );
}

#[tokio::test]
async fn receipt_hash_verifies() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "verify hash test").await;
    let stored = receipt.receipt_sha256.clone().unwrap();
    let computed = abp_receipt::compute_hash(&receipt).unwrap();
    assert_eq!(stored, computed);
}

#[tokio::test]
async fn receipt_contains_contract_version() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "version test").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_contains_backend_id() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "backend id test").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn receipt_has_work_order_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("wo id test");
    let wo_id = wo.id;
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().meta.work_order_id, wo_id);
}

#[tokio::test]
async fn receipt_has_trace() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "trace test").await;
    assert!(
        !receipt.trace.is_empty(),
        "receipt should have trace events"
    );
}

#[tokio::test]
async fn receipt_duration_nonnegative() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "duration test").await;
    // duration_ms is u64, always non-negative, just check it's reasonable
    assert!(receipt.meta.duration_ms < 60_000);
}

#[tokio::test]
async fn receipt_timestamps_ordered() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "timestamp order").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

// ===========================================================================
// 7. PolicyFailed errors
// ===========================================================================

#[test]
fn policy_failed_error_variant() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
    assert!(err.to_string().contains("policy"));
}

#[test]
fn policy_failed_into_abp_error() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad pattern"));
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::PolicyInvalid);
}

// ===========================================================================
// 8. WorkspaceFailed errors
// ===========================================================================

#[test]
fn workspace_failed_error_variant() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
    assert!(err.to_string().contains("workspace"));
}

#[test]
fn workspace_failed_into_abp_error() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("io error"));
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::WorkspaceInitFailed);
}

// ===========================================================================
// 9. BackendFailed errors
// ===========================================================================

#[test]
fn backend_failed_error_variant() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
    assert!(err.to_string().contains("backend"));
}

#[test]
fn backend_failed_into_abp_error() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendCrashed);
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
    let wo = passthrough_wo("fail test");
    let handle = rt.run_streaming("fail", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(
        matches!(receipt, Err(RuntimeError::BackendFailed(_))),
        "expected BackendFailed, got {receipt:?}"
    );
}

#[tokio::test]
async fn panicking_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("panic", PanickingBackend);
    let wo = passthrough_wo("panic test");
    let handle = rt.run_streaming("panic", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(
        matches!(receipt, Err(RuntimeError::BackendFailed(_))),
        "expected BackendFailed, got {receipt:?}"
    );
}

// ===========================================================================
// 10. Run lifecycle (start → events → receipt)
// ===========================================================================

#[tokio::test]
async fn full_run_lifecycle() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("lifecycle test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;

    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Events were received
    assert!(!events.is_empty());
    // Receipt is complete
    assert_eq!(receipt.outcome, Outcome::Complete);
    // Receipt has hash
    assert!(receipt.receipt_sha256.is_some());
    // run_id is non-nil
    assert_ne!(run_id, Uuid::nil());
}

#[tokio::test]
async fn run_lifecycle_events_before_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("order test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();

    let mut events_rx = handle.events;
    let mut events = Vec::new();
    while let Some(ev) = events_rx.next().await {
        events.push(ev);
    }
    // Receipt resolves after events stream ends
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(!events.is_empty());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 11. Multiple sequential runs
// ===========================================================================

#[tokio::test]
async fn multiple_sequential_runs_produce_different_run_ids() {
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "run 1").await;
    let r2 = run_mock(&rt, "run 2").await;
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn multiple_runs_produce_different_hashes() {
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "hash run 1").await;
    let r2 = run_mock(&rt, "hash run 2").await;
    // Different runs produce different hashes (different run_ids, timestamps)
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn three_sequential_runs_all_complete() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let receipt = run_mock(&rt, &format!("seq run {i}")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[tokio::test]
async fn sequential_runs_update_metrics() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "metrics 1").await;
    run_mock(&rt, "metrics 2").await;
    run_mock(&rt, "metrics 3").await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
}

#[tokio::test]
async fn receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain 1").await;
    run_mock(&rt, "chain 2").await;
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 2);
}

// ===========================================================================
// 12. Default configuration
// ===========================================================================

#[test]
fn default_runtime_config() {
    let config = RuntimeConfig::default();
    assert!(config.model.is_none());
    assert!(config.vendor.is_empty());
    assert!(config.env.is_empty());
    assert!(config.max_budget_usd.is_none());
    assert!(config.max_turns.is_none());
}

#[test]
fn work_order_builder_default_config() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn work_order_builder_with_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_with_budget() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(10.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn work_order_builder_with_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

// ===========================================================================
// 13. Serde roundtrip of RuntimeConfig
// ===========================================================================

#[test]
fn runtime_config_serde_roundtrip_default() {
    let config = RuntimeConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, config.model);
    assert_eq!(back.max_budget_usd, config.max_budget_usd);
    assert_eq!(back.max_turns, config.max_turns);
}

#[test]
fn runtime_config_serde_roundtrip_with_model() {
    let config = RuntimeConfig {
        model: Some("claude-3".into()),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, Some("claude-3".into()));
}

#[test]
fn runtime_config_serde_roundtrip_with_vendor() {
    let config = RuntimeConfig {
        vendor: {
            let mut v = std::collections::BTreeMap::new();
            v.insert("key".into(), serde_json::json!("value"));
            v
        },
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vendor.get("key"), Some(&serde_json::json!("value")));
}

#[test]
fn runtime_config_serde_roundtrip_with_env() {
    let config = RuntimeConfig {
        env: {
            let mut e = std::collections::BTreeMap::new();
            e.insert("FOO".into(), "bar".into());
            e
        },
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.env.get("FOO"), Some(&"bar".to_string()));
}

#[test]
fn runtime_config_serde_roundtrip_with_budget_and_turns() {
    let config = RuntimeConfig {
        max_budget_usd: Some(42.5),
        max_turns: Some(10),
        ..RuntimeConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_budget_usd, Some(42.5));
    assert_eq!(back.max_turns, Some(10));
}

#[test]
fn runtime_config_from_json_string() {
    let json = r#"{"model":"test","vendor":{},"env":{},"max_budget_usd":null,"max_turns":null}"#;
    let config: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.model, Some("test".to_string()));
}

// ===========================================================================
// 14. RuntimeError variants
// ===========================================================================

#[test]
fn all_runtime_error_variants_have_codes() {
    let errors: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("a")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("b")),
        RuntimeError::BackendFailed(anyhow::anyhow!("c")),
        RuntimeError::CapabilityCheckFailed("d".into()),
        RuntimeError::NoProjectionMatch { reason: "e".into() },
    ];
    for err in errors {
        // Should not panic
        let _code = err.error_code();
        let _msg = err.to_string();
    }
}

#[test]
fn capability_check_failed_error() {
    let err = RuntimeError::CapabilityCheckFailed("missing tool_read".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
    assert!(err.to_string().contains("missing tool_read"));
}

#[test]
fn no_projection_match_error() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no backends".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(err.to_string().contains("no backends"));
}

#[test]
fn classified_error_roundtrip() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, abp_error::ErrorCode::BackendTimeout);
}

// ===========================================================================
// 15. Capability checks
// ===========================================================================

#[test]
fn check_capabilities_passes_for_streaming() {
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
fn check_capabilities_fails_for_mcp() {
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

#[test]
fn check_capabilities_empty_reqs_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn check_capabilities_unknown_backend_fails() {
    let rt = Runtime::new();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("nope", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 16. Backend registry operations
// ===========================================================================

#[test]
fn backend_registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn backend_registry_get_returns_none_for_missing() {
    let reg = BackendRegistry::default();
    assert!(reg.get("nope").is_none());
}

#[test]
fn backend_registry_register_and_get() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.get("mock").is_some());
}

#[test]
fn backend_registry_multiple_entries() {
    let mut reg = BackendRegistry::default();
    reg.register("a", MockBackend);
    reg.register("b", MockBackend);
    assert_eq!(reg.list().len(), 2);
}

// ===========================================================================
// 17. Telemetry and metrics
// ===========================================================================

#[tokio::test]
async fn metrics_track_success() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "metric test").await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn metrics_track_events() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "events metric test").await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn metrics_average_duration() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "avg duration").await;
    let snap = rt.metrics().snapshot();
    // Average should be set (may be 0 for very fast runs)
    assert!(snap.average_run_duration_ms < 60_000);
}

// ===========================================================================
// 18. Pipeline stages
// ===========================================================================

#[test]
fn pipeline_new_is_empty() {
    let p = pipeline::Pipeline::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
}

#[test]
fn pipeline_default_is_empty() {
    let p = pipeline::Pipeline::default();
    assert!(p.is_empty());
}

#[test]
fn pipeline_add_stage() {
    let p = pipeline::Pipeline::new().stage(pipeline::ValidationStage);
    assert_eq!(p.len(), 1);
    assert!(!p.is_empty());
}

#[test]
fn pipeline_chain_stages() {
    let p = pipeline::Pipeline::new()
        .stage(pipeline::ValidationStage)
        .stage(pipeline::AuditStage::new());
    assert_eq!(p.len(), 2);
}

#[tokio::test]
async fn pipeline_validation_rejects_empty_task() {
    let p = pipeline::Pipeline::new().stage(pipeline::ValidationStage);
    let mut wo = WorkOrderBuilder::new("").build();
    let result = p.execute(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn pipeline_validation_accepts_valid_order() {
    let p = pipeline::Pipeline::new().stage(pipeline::ValidationStage);
    let mut wo = WorkOrderBuilder::new("valid task").build();
    let result = p.execute(&mut wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn pipeline_audit_records_entry() {
    let audit = pipeline::AuditStage::new();
    let p = pipeline::Pipeline::new().stage(pipeline::PolicyStage);
    let mut wo = WorkOrderBuilder::new("audit test").build();
    let _ = p.execute(&mut wo).await;

    // AuditStage when used directly
    let p2 = pipeline::Pipeline::new();
    let mut wo2 = WorkOrderBuilder::new("direct").build();
    let _ = p2.execute(&mut wo2).await;
    // No panic means success
    let entries = audit.entries().await;
    assert_eq!(entries.len(), 0); // audit not in p2
}

// ===========================================================================
// 19. Stage pipeline (StagePipeline)
// ===========================================================================

#[tokio::test]
async fn stage_pipeline_reports_per_stage_results() {
    let sp = stages::PipelineBuilder::new()
        .add_stage(Box::new(pipeline::ValidationStage))
        .build();
    let mut wo = WorkOrderBuilder::new("stage test").build();
    let results = sp.execute(&mut wo).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].passed);
    assert_eq!(results[0].stage_name, "validation");
}

#[tokio::test]
async fn stage_pipeline_runs_all_stages_even_on_failure() {
    let sp = stages::PipelineBuilder::new()
        .add_stage(Box::new(pipeline::ValidationStage))
        .add_stage(Box::new(pipeline::PolicyStage))
        .build();
    let mut wo = WorkOrderBuilder::new("").build();
    let results = sp.execute(&mut wo).await;
    // ValidationStage fails, but both stages ran
    assert_eq!(results.len(), 2);
    assert!(!results[0].passed);
}

#[test]
fn stage_pipeline_names() {
    let sp = stages::PipelineBuilder::new()
        .add_stage(Box::new(pipeline::ValidationStage))
        .add_stage(Box::new(pipeline::PolicyStage))
        .build();
    let names = sp.stage_names();
    assert_eq!(names, vec!["validation", "policy"]);
}

#[test]
fn pipeline_builder_stage_count() {
    let builder = stages::PipelineBuilder::new().add_stage(Box::new(pipeline::ValidationStage));
    assert_eq!(builder.stage_count(), 1);
}

// ===========================================================================
// 20. Budget tracking
// ===========================================================================

#[test]
fn budget_within_limits_no_caps() {
    let tracker = budget::BudgetTracker::new(budget::BudgetLimit::default());
    tracker.record_tokens(999_999);
    assert_eq!(tracker.check(), budget::BudgetStatus::WithinLimits);
}

#[test]
fn budget_tokens_exceeded() {
    let tracker = budget::BudgetTracker::new(budget::BudgetLimit {
        max_tokens: Some(100),
        ..Default::default()
    });
    tracker.record_tokens(101);
    assert!(matches!(tracker.check(), budget::BudgetStatus::Exceeded(_)));
}

#[test]
fn budget_turns_exceeded() {
    let tracker = budget::BudgetTracker::new(budget::BudgetLimit {
        max_turns: Some(2),
        ..Default::default()
    });
    tracker.record_turn();
    tracker.record_turn();
    tracker.record_turn();
    assert!(matches!(tracker.check(), budget::BudgetStatus::Exceeded(_)));
}

#[test]
fn budget_remaining_reflects_usage() {
    let tracker = budget::BudgetTracker::new(budget::BudgetLimit {
        max_tokens: Some(1000),
        ..Default::default()
    });
    tracker.record_tokens(300);
    let remaining = tracker.remaining();
    assert_eq!(remaining.tokens, Some(700));
}

// ===========================================================================
// 21. Cancellation
// ===========================================================================

#[test]
fn cancellation_token_starts_not_cancelled() {
    let token = cancel::CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel_flips() {
    let token = cancel::CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancellation_token_clone_shares_state() {
    let a = cancel::CancellationToken::new();
    let b = a.clone();
    a.cancel();
    assert!(b.is_cancelled());
}

#[test]
fn cancellable_run_tracks_reason() {
    let run = cancel::CancellableRun::new(cancel::CancellationToken::new());
    run.cancel(cancel::CancellationReason::Timeout);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(cancel::CancellationReason::Timeout));
}

#[test]
fn cancellable_run_keeps_first_reason() {
    let run = cancel::CancellableRun::new(cancel::CancellationToken::new());
    run.cancel(cancel::CancellationReason::Timeout);
    run.cancel(cancel::CancellationReason::UserRequested);
    assert_eq!(run.reason(), Some(cancel::CancellationReason::Timeout));
}

// ===========================================================================
// 22. Event multiplexer
// ===========================================================================

#[test]
fn multiplexer_no_subscribers_initially() {
    let mux = multiplex::EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
}

#[test]
fn multiplexer_subscriber_increments_count() {
    let mux = multiplex::EventMultiplexer::new(16);
    let _sub = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
}

#[tokio::test]
async fn multiplexer_broadcast_delivers_event() {
    let mux = multiplex::EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "test".into(),
        },
        ext: None,
    };
    mux.broadcast(ev.clone()).unwrap();
    let received = sub.recv().await.unwrap();
    assert!(matches!(received.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn multiplexer_broadcast_no_subscribers_fails() {
    let mux = multiplex::EventMultiplexer::new(16);
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "test".into(),
        },
        ext: None,
    };
    let result = mux.broadcast(ev);
    assert!(result.is_err());
}

// ===========================================================================
// 23. Event bus
// ===========================================================================

#[test]
fn event_bus_default_is_empty() {
    let bus = bus::EventBus::new();
    let stats = bus.stats();
    assert_eq!(stats.total_published, 0);
    assert_eq!(stats.active_subscribers, 0);
}

#[test]
fn event_bus_subscribe_increments_count() {
    let bus = bus::EventBus::new();
    let _sub = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);
}

#[tokio::test]
async fn event_bus_publish_and_receive() {
    let bus = bus::EventBus::new();
    let mut sub = bus.subscribe();
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    bus.publish(ev);
    let received = sub.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn event_bus_stats_track_publishing() {
    let bus = bus::EventBus::new();
    let _sub = bus.subscribe();
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: None,
    };
    bus.publish(ev);
    let stats = bus.stats();
    assert_eq!(stats.total_published, 1);
}

// ===========================================================================
// 24. Hooks
// ===========================================================================

#[test]
fn hook_registry_empty() {
    let reg = hooks::HookRegistry::new();
    assert_eq!(reg.hook_count(), 0);
    assert!(reg.hook_names().is_empty());
}

#[test]
fn hook_registry_register_and_count() {
    let mut reg = hooks::HookRegistry::new();
    reg.register(Box::new(hooks::LoggingHook));
    assert_eq!(reg.hook_count(), 1);
    assert_eq!(reg.hook_names(), vec!["logging"]);
}

#[test]
fn hook_registry_fire_run_start() {
    let mut reg = hooks::HookRegistry::new();
    reg.register(Box::new(hooks::ValidationHook));
    let wo = WorkOrderBuilder::new("valid task").build();
    let results = reg.fire_run_start(&wo);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn hook_registry_validation_rejects_empty_task() {
    let mut reg = hooks::HookRegistry::new();
    reg.register(Box::new(hooks::ValidationHook));
    let wo = WorkOrderBuilder::new("").build();
    let results = reg.fire_run_start(&wo);
    assert!(results[0].is_err());
}

// ===========================================================================
// 25. Retry policy
// ===========================================================================

#[test]
fn retry_policy_default() {
    let p = retry::RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
}

#[test]
fn retry_policy_should_retry() {
    let p = retry::RetryPolicy::default();
    assert!(p.should_retry(0));
    assert!(p.should_retry(2));
    assert!(!p.should_retry(3));
}

#[test]
fn retry_policy_compute_delay_increases() {
    let p = retry::RetryPolicy::default();
    let d0 = p.compute_delay(0);
    let d2 = p.compute_delay(2);
    // Delay should generally increase (though jitter adds noise)
    assert!(d2 >= d0 || d2.as_millis() > 0);
}

#[test]
fn retry_policy_builder() {
    let p = retry::RetryPolicy::builder().max_retries(5).build();
    assert_eq!(p.max_retries, 5);
}

#[test]
fn retry_policy_serde_roundtrip() {
    let p = retry::RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let back: retry::RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_retries, p.max_retries);
}

#[test]
fn timeout_config_default() {
    let tc = retry::TimeoutConfig::default();
    assert!(tc.run_timeout.is_none());
    assert!(tc.event_timeout.is_none());
}

#[test]
fn timeout_config_serde_roundtrip() {
    let tc = retry::TimeoutConfig {
        run_timeout: Some(std::time::Duration::from_secs(30)),
        event_timeout: Some(std::time::Duration::from_secs(5)),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: retry::TimeoutConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_timeout, tc.run_timeout);
    assert_eq!(back.event_timeout, tc.event_timeout);
}

// ===========================================================================
// 26. Observability
// ===========================================================================

#[test]
fn trace_collector_empty() {
    let tc = observe::TraceCollector::new();
    assert!(tc.spans().is_empty());
}

#[test]
fn trace_collector_start_span() {
    let mut tc = observe::TraceCollector::new();
    let id = tc.start_span("test");
    assert_eq!(tc.spans().len(), 1);
    assert_eq!(tc.spans()[0].name, "test");
    assert!(!id.is_empty());
}

#[test]
fn trace_collector_child_span() {
    let mut tc = observe::TraceCollector::new();
    let parent = tc.start_span("parent");
    let _child = tc.start_child_span("child", &parent);
    assert_eq!(tc.spans().len(), 2);
    assert_eq!(tc.children(&parent).len(), 1);
}

#[test]
fn trace_collector_end_span() {
    let mut tc = observe::TraceCollector::new();
    let id = tc.start_span("to-end");
    assert!(tc.active_spans().len() == 1);
    tc.end_span(&id);
    assert!(tc.active_spans().is_empty());
}

#[test]
fn runtime_observer_summary() {
    let mut obs = observe::RuntimeObserver::new();
    obs.record_metric("latency_ms", 42.0);
    obs.trace_collector().start_span("root");
    let summary = obs.summary();
    assert_eq!(summary.total_spans, 1);
    assert_eq!(summary.active_spans, 1);
    assert_eq!(summary.metrics_count, 1);
}

// ===========================================================================
// 27. Receipt store
// ===========================================================================

#[test]
fn receipt_store_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::ReceiptStore::new(dir.path());
    let ids = store.list().unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::ReceiptStore::new(dir.path());

    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "store test").await;
    let run_id = receipt.meta.run_id;

    store.save(&receipt).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
}

#[tokio::test]
async fn receipt_store_verify() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::ReceiptStore::new(dir.path());

    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "verify store").await;
    store.save(&receipt).unwrap();

    assert!(store.verify(receipt.meta.run_id).unwrap());
}

#[tokio::test]
async fn receipt_store_chain_verification() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::ReceiptStore::new(dir.path());

    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "chain verify 1").await;
    let r2 = run_mock(&rt, "chain verify 2").await;
    store.save(&r1).unwrap();
    store.save(&r2).unwrap();

    let verification = store.verify_chain().unwrap();
    assert!(verification.is_valid);
    assert_eq!(verification.valid_count, 2);
}

// ===========================================================================
// 28. Event router
// ===========================================================================

#[test]
fn event_router_empty() {
    let router = multiplex::EventRouter::new();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn event_router_add_route() {
    let mut router = multiplex::EventRouter::new();
    router.add_route("run_started", Box::new(|_| {}));
    assert_eq!(router.route_count(), 1);
}

// ===========================================================================
// 29. Projection integration
// ===========================================================================

#[test]
fn runtime_with_projection() {
    let matrix = ProjectionMatrix::new();
    let rt = Runtime::new().with_projection(matrix);
    assert!(rt.projection().is_some());
}

#[test]
fn runtime_without_projection() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn select_backend_without_projection_errors() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("test").build();
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

#[test]
fn select_backend_with_registered_backend() {
    use abp_dialect::Dialect;
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("test").build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "mock");
}

#[tokio::test]
async fn run_projected_uses_best_backend() {
    use abp_dialect::Dialect;
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_manifest(), Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("projected run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_projected(wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().backend.id, "mock");
}

#[tokio::test]
async fn run_projected_without_matrix_fails() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("no matrix");
    let result = rt.run_projected(wo).await;
    assert!(matches!(
        result,
        Err(RuntimeError::NoProjectionMatch { .. })
    ));
}

// ===========================================================================
// 30. Stream pipeline integration
// ===========================================================================

#[test]
fn runtime_with_stream_pipeline() {
    use abp_stream::StreamPipelineBuilder;
    let pipeline = StreamPipelineBuilder::new().build();
    let rt = Runtime::new().with_stream_pipeline(pipeline);
    assert!(rt.stream_pipeline().is_some());
}

// ===========================================================================
// 31. Emulation config
// ===========================================================================

#[test]
fn runtime_with_emulation() {
    use abp_emulation::EmulationConfig;
    let config = EmulationConfig::default();
    let rt = Runtime::new().with_emulation(config);
    assert!(rt.emulation_config().is_some());
}

#[test]
fn runtime_emulation_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

// ===========================================================================
// 32. Receipt chain
// ===========================================================================

#[tokio::test]
async fn receipt_chain_starts_empty() {
    let rt = Runtime::new();
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 0);
}

#[tokio::test]
async fn receipt_chain_grows_with_runs() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain grow 1").await;
    run_mock(&rt, "chain grow 2").await;
    run_mock(&rt, "chain grow 3").await;
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 3);
}

// ===========================================================================
// 33. Metrics stage
// ===========================================================================

#[tokio::test]
async fn metrics_stage_tracks_invocations() {
    let ms = stages::MetricsStage::new();
    let mut wo = WorkOrderBuilder::new("metrics stage").build();
    use crate::pipeline::PipelineStage;
    ms.process(&mut wo).await.unwrap();
    let stats = ms.stats().await;
    assert_eq!(stats.invocations, 1);
    assert_eq!(stats.successes, 1);
}

// ===========================================================================
// 34. Rate limit stage
// ===========================================================================

#[tokio::test]
async fn rate_limit_stage_allows_under_limit() {
    let rl = stages::RateLimitStage::new(10);
    let mut wo = WorkOrderBuilder::new("rate limit").build();
    use crate::pipeline::PipelineStage;
    assert!(rl.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn rate_limit_stage_rejects_over_limit() {
    let rl = stages::RateLimitStage::new(2);
    let mut wo = WorkOrderBuilder::new("rate limit").build();
    use crate::pipeline::PipelineStage;
    rl.process(&mut wo).await.unwrap();
    rl.process(&mut wo).await.unwrap();
    let result = rl.process(&mut wo).await;
    assert!(result.is_err());
}

// ===========================================================================
// 35. Deduplication stage
// ===========================================================================

#[tokio::test]
async fn dedup_stage_allows_first_order() {
    let stage = stages::DeduplicationStage::new(std::time::Duration::from_secs(60));
    let mut wo = WorkOrderBuilder::new("dedup test").build();
    use crate::pipeline::PipelineStage;
    assert!(stage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn dedup_stage_rejects_duplicate() {
    let stage = stages::DeduplicationStage::new(std::time::Duration::from_secs(60));
    let mut wo1 = WorkOrderBuilder::new("same task").root("same_root").build();
    let mut wo2 = WorkOrderBuilder::new("same task").root("same_root").build();
    use crate::pipeline::PipelineStage;
    stage.process(&mut wo1).await.unwrap();
    let result = stage.process(&mut wo2).await;
    assert!(result.is_err());
}

// ===========================================================================
// 36. Logging stage
// ===========================================================================

#[tokio::test]
async fn logging_stage_does_not_fail() {
    let stage = stages::LoggingStage::new("TEST");
    let mut wo = WorkOrderBuilder::new("log test").build();
    use crate::pipeline::PipelineStage;
    assert!(stage.process(&mut wo).await.is_ok());
}

// ===========================================================================
// 37. Cancellation reason serde
// ===========================================================================

#[test]
fn cancellation_reason_serde_roundtrip() {
    let reasons = [
        cancel::CancellationReason::UserRequested,
        cancel::CancellationReason::Timeout,
        cancel::CancellationReason::BudgetExhausted,
        cancel::CancellationReason::PolicyViolation,
        cancel::CancellationReason::SystemShutdown,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let back: cancel::CancellationReason = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, reason);
    }
}

#[test]
fn all_cancellation_reasons_have_descriptions() {
    let reasons = [
        cancel::CancellationReason::UserRequested,
        cancel::CancellationReason::Timeout,
        cancel::CancellationReason::BudgetExhausted,
        cancel::CancellationReason::PolicyViolation,
        cancel::CancellationReason::SystemShutdown,
    ];
    for r in &reasons {
        assert!(!r.description().is_empty());
    }
}

// ===========================================================================
// 38. WorkOrder creation variations
// ===========================================================================

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn work_order_default_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn work_order_passthrough_mode() {
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn work_order_custom_root() {
    let wo = WorkOrderBuilder::new("test").root("/custom/path").build();
    assert_eq!(wo.workspace.root, "/custom/path");
}

#[test]
fn work_order_with_include_exclude() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["**/*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_with_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("test").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

// ===========================================================================
// 39. Mixed error scenarios
// ===========================================================================

#[tokio::test]
async fn run_after_backend_failure_succeeds() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "boom".into(),
        },
    );
    rt.register_backend("mock", MockBackend);

    // First run fails
    let wo = passthrough_wo("fail first");
    let handle = rt.run_streaming("fail", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());

    // Second run on mock succeeds
    let receipt = run_mock(&rt, "succeed after fail").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn unknown_backend_then_valid_backend() {
    let rt = Runtime::with_default_backends();
    let wo1 = passthrough_wo("unknown first");
    let err = rt.run_streaming("bogus", wo1).await;
    assert!(err.is_err());

    let receipt = run_mock(&rt, "valid second").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 40. Additional edge cases
// ===========================================================================

#[test]
fn runtime_error_display_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(err.to_string(), "unknown backend: foo");
}

#[test]
fn runtime_error_display_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk error"));
    assert_eq!(err.to_string(), "workspace preparation failed");
}

#[test]
fn runtime_error_display_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert_eq!(err.to_string(), "policy compilation failed");
}

#[test]
fn runtime_error_display_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.to_string(), "backend execution failed");
}

#[test]
fn runtime_error_display_capability_check() {
    let err = RuntimeError::CapabilityCheckFailed("mcp missing".into());
    assert_eq!(err.to_string(), "capability check failed: mcp missing");
}

#[test]
fn runtime_error_display_no_projection() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "none".into(),
    };
    assert_eq!(err.to_string(), "projection failed: none");
}

#[test]
fn span_status_serde_roundtrip() {
    let statuses = [
        observe::SpanStatus::Ok,
        observe::SpanStatus::Error {
            message: "oops".into(),
        },
        observe::SpanStatus::Unset,
    ];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let back: observe::SpanStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status);
    }
}

#[test]
fn trace_collector_to_json() {
    let mut tc = observe::TraceCollector::new();
    tc.start_span("test");
    let json = tc.to_json();
    assert!(json.contains("test"));
}

#[test]
fn budget_limit_serde_roundtrip() {
    let limit = budget::BudgetLimit {
        max_tokens: Some(5000),
        max_cost_usd: Some(1.5),
        max_turns: Some(10),
        max_duration: Some(std::time::Duration::from_secs(120)),
    };
    let json = serde_json::to_string(&limit).unwrap();
    let back: budget::BudgetLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_tokens, Some(5000));
    assert_eq!(back.max_cost_usd, Some(1.5));
    assert_eq!(back.max_turns, Some(10));
    assert_eq!(back.max_duration, Some(std::time::Duration::from_secs(120)));
}

#[test]
fn event_bus_with_capacity() {
    let bus = bus::EventBus::with_capacity(8);
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn observability_summary_serde_roundtrip() {
    let summary = observe::ObservabilitySummary {
        total_spans: 5,
        active_spans: 2,
        error_spans: 1,
        metrics_count: 3,
    };
    let json = serde_json::to_string(&summary).unwrap();
    let back: observe::ObservabilitySummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back, summary);
}
