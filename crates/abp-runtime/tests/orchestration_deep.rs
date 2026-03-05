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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
//! Deep orchestration tests for the ABP runtime pipeline.
//!
//! 80+ tests covering: full pipeline, backend selection, event multiplexing,
//! receipt generation, workspace staging, policy enforcement, error propagation,
//! unknown backend handling, workspace failure handling, policy failure handling,
//! RunHandle API, concurrent runs, cancellation, and configuration passthrough.

use std::collections::HashSet;
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ExecutionLane, MinSupport, Outcome, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};
use abp_runtime::multiplex::EventMultiplexer;
use abp_runtime::pipeline::{Pipeline, PolicyStage, ValidationStage};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{Runtime, RuntimeError};
use chrono::Utc;
use tokio_stream::StreamExt;

// ── Helpers ────────────────────────────────────────────────────────────

fn wo(task: &str) -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: task.into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn wo_with_policy(task: &str, policy: PolicyProfile) -> WorkOrder {
    let mut w = wo(task);
    w.policy = policy;
    w
}

fn wo_with_config(task: &str, config: RuntimeConfig) -> WorkOrder {
    let mut w = wo(task);
    w.config = config;
    w
}

fn wo_with_requirements(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    let mut w = wo(task);
    w.requirements = reqs;
    w
}

async fn drain(handle: abp_runtime::RunHandle) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

async fn drain_events_only(handle: abp_runtime::RunHandle) -> Vec<AgentEvent> {
    let events: Vec<_> = handle.events.collect().await;
    let _ = handle.receipt.await;
    events
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Full pipeline (workspace → backend → events → receipt)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn full_pipeline_receipt_has_valid_hash() {
    let rt = Runtime::with_default_backends();
    let w = wo("full pipeline hash");
    let wo_id = w.id;

    let (events, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(matches!(receipt.outcome, Outcome::Complete));
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn full_pipeline_events_include_start_and_complete() {
    let rt = Runtime::with_default_backends();
    let (events, _) = drain(
        rt.run_streaming("mock", wo("start/complete"))
            .await
            .unwrap(),
    )
    .await;

    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. })));
}

#[tokio::test]
async fn full_pipeline_receipt_trace_non_empty() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("trace")).await.unwrap()).await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn full_pipeline_receipt_outcome_is_complete() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("outcome")).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn full_pipeline_events_contain_assistant_messages() {
    let rt = Runtime::with_default_backends();
    let (events, _) = drain(rt.run_streaming("mock", wo("assistant msg")).await.unwrap()).await;
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. })));
}

#[tokio::test]
async fn full_pipeline_receipt_backend_identity() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("identity")).await.unwrap()).await;
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn full_pipeline_receipt_duration_is_nonnegative() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("duration")).await.unwrap()).await;
    // duration_ms is u64 so always >= 0, just verify it's set
    let _ = receipt.meta.duration_ms;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn full_pipeline_receipt_has_sha256_of_64_hex_chars() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("hex")).await.unwrap()).await;
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backend selection (by name, mock, multiple backends)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_selection_mock_is_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_selection_names_contains_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn backend_selection_register_custom_name() {
    let mut rt = Runtime::new();
    rt.register_backend("custom", abp_integrations::MockBackend);
    assert!(rt.backend("custom").is_some());
    assert!(rt.backend_names().contains(&"custom".to_string()));
}

#[test]
fn backend_selection_register_replaces_existing() {
    let mut rt = Runtime::new();
    rt.register_backend("b1", abp_integrations::MockBackend);
    rt.register_backend("b1", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[tokio::test]
async fn backend_selection_run_on_named_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", abp_integrations::MockBackend);
    let (_, receipt) = drain(rt.run_streaming("alpha", wo("alpha")).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[test]
fn backend_selection_multiple_backends_listed() {
    let mut rt = Runtime::new();
    rt.register_backend("a", abp_integrations::MockBackend);
    rt.register_backend("b", abp_integrations::MockBackend);
    rt.register_backend("c", abp_integrations::MockBackend);
    let names = rt.backend_names();
    assert_eq!(names.len(), 3);
}

#[tokio::test]
async fn backend_selection_run_each_of_multiple() {
    let mut rt = Runtime::new();
    for name in &["x", "y", "z"] {
        rt.register_backend(name, abp_integrations::MockBackend);
    }
    for name in &["x", "y", "z"] {
        let (_, receipt) = drain(rt.run_streaming(name, wo(name)).await.unwrap()).await;
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[test]
fn backend_selection_registry_contains() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

#[test]
fn backend_selection_registry_remove() {
    let mut rt = Runtime::new();
    rt.register_backend("removable", abp_integrations::MockBackend);
    assert!(rt.registry().contains("removable"));
    let removed = rt.registry_mut().remove("removable");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("removable"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Event multiplexing (events forwarded to channels correctly)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn mux_single_subscriber_receives_event() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    }))
    .unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn mux_multiple_subscribers_all_receive() {
    let mux = Arc::new(EventMultiplexer::new(32));
    let mut subs: Vec<_> = (0..5).map(|_| mux.subscribe()).collect();

    mux.broadcast(make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    }))
    .unwrap();

    for sub in &mut subs {
        let ev = sub.recv().await.unwrap();
        assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
    }
}

#[tokio::test]
async fn mux_sequential_events_arrive_in_order() {
    let mux = EventMultiplexer::new(32);
    let mut sub = mux.subscribe();

    let kinds = [
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        AgentEventKind::AssistantMessage { text: "b".into() },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
    ];
    for k in &kinds {
        mux.broadcast(make_event(k.clone())).unwrap();
    }

    assert!(matches!(
        sub.recv().await.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        sub.recv().await.unwrap().kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        sub.recv().await.unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn mux_broadcast_returns_subscriber_count() {
    let mux = EventMultiplexer::new(32);
    let _s1 = mux.subscribe();
    let _s2 = mux.subscribe();
    let _s3 = mux.subscribe();
    let count = mux
        .broadcast(make_event(AgentEventKind::RunStarted {
            message: "count".into(),
        }))
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn mux_concurrent_subscribers_receive() {
    let mux = Arc::new(EventMultiplexer::new(64));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let mut sub = mux.subscribe();
        handles.push(tokio::spawn(async move {
            sub.recv().await.unwrap();
            true
        }));
    }

    tokio::task::yield_now().await;
    mux.broadcast(make_event(AgentEventKind::RunStarted {
        message: "concurrent".into(),
    }))
    .unwrap();

    for h in handles {
        assert!(h.await.unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Receipt generation (hash, timestamps, contract_version)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn receipt_contract_version_matches() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("cv")).await.unwrap()).await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_work_order_id_matches() {
    let rt = Runtime::with_default_backends();
    let w = wo("wo-id");
    let expected_id = w.id;
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert_eq!(receipt.meta.work_order_id, expected_id);
}

#[tokio::test]
async fn receipt_run_id_is_nonzero() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("run-id")).await.unwrap()).await;
    assert!(!receipt.meta.run_id.is_nil());
}

#[tokio::test]
async fn receipt_timestamps_are_utc() {
    let rt = Runtime::with_default_backends();
    let before = Utc::now();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("ts")).await.unwrap()).await;
    let after = Utc::now();
    assert!(receipt.meta.started_at >= before);
    assert!(receipt.meta.finished_at <= after);
}

#[tokio::test]
async fn receipt_hash_is_deterministic_for_same_receipt() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("det")).await.unwrap()).await;
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn receipt_different_runs_have_different_hashes() {
    let rt = Runtime::with_default_backends();
    let (_, r1) = drain(rt.run_streaming("mock", wo("hash1")).await.unwrap()).await;
    let (_, r2) = drain(rt.run_streaming("mock", wo("hash2")).await.unwrap()).await;
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn receipt_capabilities_manifest_nonempty() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain(rt.run_streaming("mock", wo("caps")).await.unwrap()).await;
    assert!(!receipt.capabilities.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Workspace staging (correct temp dir creation/cleanup)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn workspace_passthrough_succeeds() {
    let rt = Runtime::with_default_backends();
    let w = wo("passthrough ws");
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn workspace_staged_with_valid_root_succeeds() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("staged ws");
    w.workspace.mode = WorkspaceMode::Staged;
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn workspace_staged_temp_dir_cleaned_after_run() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("staged cleanup");
    w.workspace.mode = WorkspaceMode::Staged;
    let handle = rt.run_streaming("mock", w).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    // After the run completes, the PreparedWorkspace is dropped, which cleans
    // up the temp dir. We simply verify the run succeeded.
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn workspace_staged_with_include_exclude() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("staged globs");
    w.workspace.mode = WorkspaceMode::Staged;
    w.workspace.include = vec!["**/*.rs".into()];
    w.workspace.exclude = vec!["target/**".into()];
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn workspace_invalid_root_returns_workspace_failed() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("bad root");
    w.workspace.mode = WorkspaceMode::Staged;
    w.workspace.root = "/nonexistent/path/that/does/not/exist/12345".into();
    let handle = rt.run_streaming("mock", w).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let result = handle.receipt.await.unwrap();
    match result {
        Err(RuntimeError::WorkspaceFailed(_)) => {} // expected
        Err(other) => panic!("expected WorkspaceFailed, got {other:?}"),
        Ok(_) => panic!("expected error for invalid workspace root"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Policy enforcement (denied tools/paths fail before execution)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_engine_denies_disallowed_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
}

#[test]
fn policy_engine_allows_unlisted_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Read");
    assert!(decision.allowed);
}

#[test]
fn policy_engine_denies_write_to_protected_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(std::path::Path::new(".git/config"));
    assert!(!decision.allowed);
}

#[test]
fn policy_engine_denies_read_of_secret_file() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(std::path::Path::new(".env"));
    assert!(!decision.allowed);
}

#[test]
fn policy_engine_allows_read_of_normal_file() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(std::path::Path::new("src/main.rs"));
    assert!(decision.allowed);
}

#[tokio::test]
async fn policy_pipeline_rejects_conflicting_tools() {
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);
    let mut w = wo_with_policy(
        "conflict",
        PolicyProfile {
            allowed_tools: vec!["Bash".into()],
            disallowed_tools: vec!["Bash".into()],
            ..Default::default()
        },
    );
    assert!(pipeline.execute(&mut w).await.is_err());
}

#[tokio::test]
async fn policy_default_profile_passes_pipeline() {
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);
    let mut w = wo("default policy");
    pipeline.execute(&mut w).await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Error propagation (backend errors → RuntimeError variants)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn error_unknown_backend_variant() {
    let rt = Runtime::with_default_backends();
    let result = rt.run_streaming("no_such_backend", wo("err")).await;
    match result {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "no_such_backend");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn error_capability_check_failed_variant() {
    let rt = Runtime::with_default_backends();
    let w = wo_with_requirements(
        "cap fail",
        CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        },
    );
    let result = rt.run_streaming("mock", w).await;
    match result {
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("capability") || msg.contains("Capability"),
                "error should mention capability: {msg}"
            );
        }
        Ok(_) => panic!("expected capability error"),
    }
}

#[test]
fn error_unknown_backend_error_code() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    assert!(matches!(
        err.error_code(),
        abp_error::ErrorCode::BackendNotFound
    ));
}

#[test]
fn error_workspace_failed_error_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("oops"));
    assert!(matches!(
        err.error_code(),
        abp_error::ErrorCode::WorkspaceInitFailed
    ));
}

#[test]
fn error_policy_failed_error_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad globs"));
    assert!(matches!(
        err.error_code(),
        abp_error::ErrorCode::PolicyInvalid
    ));
}

#[test]
fn error_backend_failed_is_retryable() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
    assert!(err.is_retryable());
}

#[test]
fn error_workspace_failed_is_retryable() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.is_retryable());
}

#[test]
fn error_unknown_backend_not_retryable() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    assert!(!err.is_retryable());
}

#[test]
fn error_policy_failed_not_retryable() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert!(!err.is_retryable());
}

#[test]
fn error_capability_check_not_retryable() {
    let err = RuntimeError::CapabilityCheckFailed("nope".into());
    assert!(!err.is_retryable());
}

#[test]
fn error_into_abp_error_conversion() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let abp_err = err.into_abp_error();
    assert!(matches!(
        abp_err.code,
        abp_error::ErrorCode::BackendNotFound
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Unknown backend handling (RuntimeError::UnknownBackend)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn unknown_backend_empty_runtime() {
    let rt = Runtime::new();
    match rt.run_streaming("anything", wo("u")).await {
        Err(RuntimeError::UnknownBackend { name }) => assert_eq!(name, "anything"),
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected UnknownBackend error"),
    }
}

#[tokio::test]
async fn unknown_backend_with_default_backends() {
    let rt = Runtime::with_default_backends();
    match rt.run_streaming("sidecar:nonexistent", wo("u2")).await {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "sidecar:nonexistent");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected UnknownBackend error"),
    }
}

#[tokio::test]
async fn unknown_backend_after_removal() {
    let mut rt = Runtime::new();
    rt.register_backend("temp_be", abp_integrations::MockBackend);
    rt.registry_mut().remove("temp_be");
    match rt.run_streaming("temp_be", wo("u3")).await {
        Err(RuntimeError::UnknownBackend { name }) => assert_eq!(name, "temp_be"),
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected UnknownBackend error"),
    }
}

#[test]
fn unknown_backend_error_display() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(format!("{err}"), "unknown backend: foo");
}

#[test]
fn unknown_backend_check_capabilities_returns_error() {
    let rt = Runtime::with_default_backends();
    let result = rt.check_capabilities("nonexistent", &CapabilityRequirements::default());
    match result {
        Err(RuntimeError::UnknownBackend { name }) => assert_eq!(name, "nonexistent"),
        other => panic!("expected UnknownBackend, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Workspace failure handling (RuntimeError::WorkspaceFailed)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn workspace_failure_staged_nonexistent_root() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("ws-fail");
    w.workspace.mode = WorkspaceMode::Staged;
    w.workspace.root = "/does/not/exist/zzz/qqq/11111".into();

    let handle = rt.run_streaming("mock", w).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    match handle.receipt.await.unwrap() {
        Err(RuntimeError::WorkspaceFailed(e)) => {
            let msg = format!("{e:?}");
            assert!(!msg.is_empty());
        }
        Err(other) => panic!("expected WorkspaceFailed, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk error"));
    assert_eq!(format!("{err}"), "workspace preparation failed");
}

#[test]
fn workspace_failed_source_chain() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("root cause"));
    let source = std::error::Error::source(&err);
    assert!(source.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Policy failure handling (RuntimeError::PolicyFailed)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(format!("{err}"), "policy compilation failed");
}

#[test]
fn policy_failed_not_retryable_direct() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("x"));
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn policy_invalid_glob_produces_policy_failed() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("bad glob");
    w.policy.deny_read = vec!["[invalid".into()];

    let handle = rt.run_streaming("mock", w).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    match handle.receipt.await.unwrap() {
        Err(RuntimeError::PolicyFailed(_)) => {} // expected
        Err(other) => panic!("expected PolicyFailed, got {other:?}"),
        Ok(_) => panic!("expected policy error for invalid glob"),
    }
}

#[tokio::test]
async fn policy_invalid_write_glob_produces_policy_failed() {
    let rt = Runtime::with_default_backends();
    let mut w = wo("bad write glob");
    w.policy.deny_write = vec!["[unclosed".into()];

    let handle = rt.run_streaming("mock", w).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    match handle.receipt.await.unwrap() {
        Err(RuntimeError::PolicyFailed(_)) => {}
        Err(other) => panic!("expected PolicyFailed, got {other:?}"),
        Ok(_) => panic!("expected policy error"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. RunHandle API (receipt(), events stream)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_handle_run_id_is_set() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("handle id")).await.unwrap();
    assert!(!handle.run_id.is_nil());
    let _ = drain_events_only(handle).await;
}

#[tokio::test]
async fn run_handle_events_stream_is_finite() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("finite")).await.unwrap();
    let events: Vec<_> = handle.events.collect().await;
    assert!(!events.is_empty());
    let _ = handle.receipt.await;
}

#[tokio::test]
async fn run_handle_receipt_is_joinable() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("joinable")).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let result = handle.receipt.await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_handle_events_then_receipt_ordering() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("ordering")).await.unwrap();
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    // Events should have been collected before receipt resolves
    assert!(!events.is_empty());
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn run_handle_run_id_matches_receipt() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("id match")).await.unwrap();
    let run_id = handle.run_id;
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Concurrent runs (multiple runs simultaneously)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runs_all_complete() {
    let rt = Arc::new(Runtime::with_default_backends());
    let count = 10;
    let mut handles = Vec::with_capacity(count);

    for i in 0..count {
        let rt = Arc::clone(&rt);
        handles.push(tokio::spawn(async move {
            let w = wo(&format!("concurrent-{i}"));
            let handle = rt.run_streaming("mock", w).await.unwrap();
            let run_id = handle.run_id;
            let _: Vec<_> = handle.events.collect().await;
            let receipt = handle.receipt.await.unwrap().unwrap();
            (run_id, receipt)
        }));
    }

    let mut ids = HashSet::new();
    for h in handles {
        let (run_id, receipt) = h.await.unwrap();
        ids.insert(run_id);
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(receipt.receipt_sha256.is_some());
    }
    assert_eq!(ids.len(), count);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runs_unique_work_order_ids() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut tasks = Vec::new();

    for i in 0..5 {
        let rt = Arc::clone(&rt);
        tasks.push(tokio::spawn(async move {
            let w = wo(&format!("woid-{i}"));
            let woid = w.id;
            let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
            (woid, receipt.meta.work_order_id)
        }));
    }

    let mut woset = HashSet::new();
    for t in tasks {
        let (expected, actual) = t.await.unwrap();
        assert_eq!(expected, actual);
        woset.insert(expected);
    }
    assert_eq!(woset.len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runs_telemetry_consistent() {
    let rt = Arc::new(Runtime::with_default_backends());
    let n = 8;
    let mut tasks = Vec::new();

    for i in 0..n {
        let rt = Arc::clone(&rt);
        tasks.push(tokio::spawn(async move {
            drain(
                rt.run_streaming("mock", wo(&format!("tel-{i}")))
                    .await
                    .unwrap(),
            )
            .await;
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, n as u64);
    assert_eq!(snap.successful_runs, n as u64);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_runs_different_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("m1", abp_integrations::MockBackend);
    rt.register_backend("m2", abp_integrations::MockBackend);
    let rt = Arc::new(rt);

    let backends = ["m1", "m2", "m1", "m2"];
    let mut tasks = Vec::new();

    for (i, be) in backends.iter().enumerate() {
        let rt = Arc::clone(&rt);
        let backend = be.to_string();
        tasks.push(tokio::spawn(async move {
            let (_, receipt) = drain(
                rt.run_streaming(&backend, wo(&format!("mb-{i}")))
                    .await
                    .unwrap(),
            )
            .await;
            receipt
        }));
    }

    for t in tasks {
        let receipt = t.await.unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Cancellation (dropping RunHandle, CancellationToken)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancellation_token_starts_uncancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel_sets_state() {
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

#[tokio::test]
async fn cancellation_future_resolves_after_cancel() {
    let token = CancellationToken::new();
    let c = token.clone();
    let h = tokio::spawn(async move {
        c.cancelled().await;
        true
    });
    tokio::task::yield_now().await;
    token.cancel();
    assert!(h.await.unwrap());
}

#[test]
fn cancellable_run_tracks_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::UserRequested);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(CancellationReason::UserRequested));
}

#[test]
fn cancellable_run_first_reason_wins() {
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::BudgetExhausted);
    run.cancel(CancellationReason::Timeout);
    assert_eq!(run.reason(), Some(CancellationReason::BudgetExhausted));
}

#[tokio::test]
async fn drop_run_handle_does_not_panic() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo("drop")).await.unwrap();
    // Drop handle without consuming events or receipt
    drop(handle);
    // If we get here without panic, the test passes
}

#[test]
fn cancellation_all_reasons_have_descriptions() {
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

// ═══════════════════════════════════════════════════════════════════════
// 14. Configuration passthrough (vendor config forwarded to backend)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn config_model_set_in_work_order() {
    let rt = Runtime::with_default_backends();
    let w = WorkOrderBuilder::new("config model")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4")
        .build();
    assert_eq!(w.config.model.as_deref(), Some("gpt-4"));
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn config_vendor_data_passthrough() {
    let rt = Runtime::with_default_backends();
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("custom_flag".into(), serde_json::Value::Bool(true));
    let w = wo_with_config("vendor data", config);
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn config_env_vars_passthrough() {
    let rt = Runtime::with_default_backends();
    let mut config = RuntimeConfig::default();
    config.env.insert("MY_VAR".into(), "my_value".into());
    let w = wo_with_config("env vars", config);
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn config_max_turns_passthrough() {
    let rt = Runtime::with_default_backends();
    let w = WorkOrderBuilder::new("max turns")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_turns(5)
        .build();
    assert_eq!(w.config.max_turns, Some(5));
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn config_max_budget_passthrough() {
    let rt = Runtime::with_default_backends();
    let w = WorkOrderBuilder::new("budget")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_budget_usd(10.0)
        .build();
    assert_eq!(w.config.max_budget_usd, Some(10.0));
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn config_abp_mode_in_vendor_config() {
    let rt = Runtime::with_default_backends();
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    let w = wo_with_config("abp mode", config);
    let (_, receipt) = drain(rt.run_streaming("mock", w).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ═══════════════════════════════════════════════════════════════════════
// Additional coverage: telemetry, registry, pipeline, receipt chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn telemetry_metrics_zero_initialized() {
    let metrics = RunMetrics::new();
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
}

#[test]
fn telemetry_metrics_record_success() {
    let metrics = RunMetrics::new();
    metrics.record_run(100, true, 5);
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.total_events, 5);
}

#[test]
fn telemetry_metrics_record_failure() {
    let metrics = RunMetrics::new();
    metrics.record_run(50, false, 3);
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 3);
}

#[tokio::test]
async fn telemetry_after_full_run() {
    let rt = Runtime::with_default_backends();
    drain(rt.run_streaming("mock", wo("tel-run")).await.unwrap()).await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn receipt_chain_grows_with_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        drain(
            rt.run_streaming("mock", wo(&format!("chain-{i}")))
                .await
                .unwrap(),
        )
        .await;
    }
    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert_eq!(locked.len(), 3);
}

#[tokio::test]
async fn receipt_chain_hashes_are_all_valid() {
    let rt = Runtime::with_default_backends();
    let mut receipts = Vec::new();
    for i in 0..3 {
        let (_, r) = drain(
            rt.run_streaming("mock", wo(&format!("cv-{i}")))
                .await
                .unwrap(),
        )
        .await;
        receipts.push(r);
    }
    for r in &receipts {
        let hash = r.receipt_sha256.as_ref().unwrap();
        let recomputed = abp_core::receipt_hash(r).unwrap();
        assert_eq!(hash, &recomputed);
    }
}

#[tokio::test]
async fn pipeline_validation_rejects_empty_task() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut w = wo("");
    assert!(pipeline.execute(&mut w).await.is_err());
}

#[tokio::test]
async fn pipeline_validation_accepts_nonempty_task() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut w = wo("valid task");
    pipeline.execute(&mut w).await.unwrap();
}

#[test]
fn runtime_default_has_no_projection() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_default_has_no_emulation() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_default_has_no_stream_pipeline() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[tokio::test]
async fn check_capabilities_passes_for_satisfiable() {
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
async fn check_capabilities_fails_for_unsatisfiable() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    match rt.check_capabilities("mock", &reqs) {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(msg.contains("mock"));
        }
        other => panic!("expected CapabilityCheckFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn sequential_runs_dont_leak_state() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let (events, receipt) = drain(
            rt.run_streaming("mock", wo(&format!("leak-{i}")))
                .await
                .unwrap(),
        )
        .await;
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(!events.is_empty());
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 5);
}

#[tokio::test]
async fn run_after_error_succeeds() {
    let rt = Runtime::with_default_backends();
    // First: error
    let _ = rt.run_streaming("nonexistent", wo("fail")).await;
    // Then: success
    let (_, receipt) = drain(rt.run_streaming("mock", wo("after-err")).await.unwrap()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[test]
fn no_projection_match_error_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert_eq!(format!("{err}"), "projection failed: no matrix");
}

#[test]
fn no_projection_match_not_retryable() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "test".into(),
    };
    assert!(!err.is_retryable());
}
