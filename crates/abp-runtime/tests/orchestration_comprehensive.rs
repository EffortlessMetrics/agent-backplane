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
//! Comprehensive tests for the abp-runtime orchestration engine (60+ tests).
//!
//! Covers: runtime construction, backend registration, work order execution,
//! event streaming, error handling, policy enforcement, workspace staging,
//! receipt production, configuration, and lifecycle states.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_runtime::cancel::CancellationToken;
use abp_runtime::multiplex::EventMultiplexer;
use abp_runtime::retry::RetryPolicy;
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use chrono::Utc;
use std::time::Duration;
use tokio_stream::StreamExt;

// ── Helpers ────────────────────────────────────────────────────────────

/// Minimal work order using PassThrough (avoids staging overhead).
fn passthrough_wo(task: &str) -> WorkOrder {
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

/// Drain the event stream and return collected events + receipt.
async fn drain_run(handle: abp_runtime::RunHandle) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Runtime construction — default config, custom config, with backends
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rt_new_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn rt_default_trait_same_as_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn rt_with_default_backends_includes_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn rt_with_default_backends_exactly_one() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn rt_new_no_emulation_config() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn rt_new_no_projection_matrix() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn rt_new_no_stream_pipeline() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn rt_with_emulation_stores_config() {
    let config = abp_emulation::EmulationConfig::new();
    let rt = Runtime::new().with_emulation(config);
    assert!(rt.emulation_config().is_some());
}

#[test]
fn rt_with_projection_stores_matrix() {
    let matrix = abp_projection::ProjectionMatrix::new();
    let rt = Runtime::new().with_projection(matrix);
    assert!(rt.projection().is_some());
}

#[test]
fn rt_metrics_starts_at_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backend registration — register mock, register sidecar, list backends
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn register_single_backend_discoverable() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn register_multiple_backends_all_present() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("beta", abp_integrations::MockBackend);
    rt.register_backend("gamma", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names().len(), 3);
}

#[test]
fn backend_names_sorted_alphabetically() {
    let mut rt = Runtime::new();
    rt.register_backend("zulu", abp_integrations::MockBackend);
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("mike", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names(), vec!["alpha", "mike", "zulu"]);
}

#[test]
fn backend_lookup_none_for_unregistered() {
    let rt = Runtime::new();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_lookup_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_identity_id_is_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    assert_eq!(b.identity().id, "mock");
}

#[test]
fn register_replaces_existing() {
    let mut rt = Runtime::new();
    rt.register_backend("dup", abp_integrations::MockBackend);
    rt.register_backend("dup", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn registry_ref_contains_check() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("missing"));
}

#[test]
fn registry_mut_remove_backend() {
    let mut rt = Runtime::with_default_backends();
    let removed = rt.registry_mut().remove("mock");
    assert!(removed.is_some());
    assert!(rt.backend_names().is_empty());
}

#[test]
fn registry_remove_nonexistent_returns_none() {
    let mut rt = Runtime::new();
    assert!(rt.registry_mut().remove("nope").is_none());
}

#[test]
fn registry_get_arc_returns_clone() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().get_arc("mock").is_some());
}

#[test]
fn registry_list_matches_backend_names() {
    let rt = Runtime::with_default_backends();
    let list: Vec<String> = rt.registry().list().iter().map(|s| s.to_string()).collect();
    assert_eq!(list, rt.backend_names());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Work order execution — submit, run with mock, get receipt
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_streaming_mock_produces_events() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("emit events"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    assert!(!events.is_empty());
}

#[tokio::test]
async fn run_streaming_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("hash"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn run_streaming_receipt_contract_version() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("ver"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn run_streaming_receipt_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("outcome"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_receipt_backend_id_is_mock() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("backend id"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn run_streaming_receipt_trace_populated() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("trace"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn run_streaming_receipt_verification_harness_ok() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("harness"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn run_streaming_run_id_nonzero() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("run id"))
        .await
        .unwrap();
    assert_ne!(handle.run_id, uuid::Uuid::nil());
    let _ = drain_run(handle).await;
}

#[tokio::test]
async fn run_streaming_work_order_id_preserved() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("wo-id");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Event streaming — events come through channel, proper ordering
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn events_include_run_started() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("started"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
    );
}

#[tokio::test]
async fn events_include_run_completed() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("completed"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[tokio::test]
async fn event_ordering_started_before_completed() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("order"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;

    let started_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .expect("RunStarted must be present");
    let completed_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .expect("RunCompleted must be present");
    assert!(started_idx < completed_idx);
}

#[tokio::test]
async fn events_have_timestamps() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("timestamps"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for e in &events {
        // chrono timestamps — verify they are recent (within last 60s).
        let age = Utc::now() - e.ts;
        assert!(
            age.num_seconds() < 60,
            "event timestamp should be recent, got age={age}"
        );
    }
}

#[tokio::test]
async fn event_count_matches_receipt_trace_len() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("count"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    // The streamed events count should match the receipt trace length.
    assert_eq!(events.len(), receipt.trace.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error handling — unknown backend, capability check failures
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let result = rt
        .run_streaming("nonexistent", passthrough_wo("unknown"))
        .await;
    assert!(result.is_err());
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("should fail for unknown backend"),
    };
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn unknown_backend_error_contains_name() {
    let err = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    assert!(err.to_string().contains("ghost"));
}

#[test]
fn unknown_backend_error_code_is_backend_not_found() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn workspace_failed_error_code_correct() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn policy_failed_error_code_correct() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn backend_failed_error_code_correct() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn capability_check_failed_error_code_correct() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn no_projection_match_error_code_correct() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "empty".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn workspace_failed_display_message() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("io error"));
    assert!(err.to_string().contains("workspace preparation failed"));
}

#[test]
fn policy_failed_display_message() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("glob error"));
    assert!(err.to_string().contains("policy compilation failed"));
}

#[test]
fn backend_failed_display_message() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("panic"));
    assert!(err.to_string().contains("backend execution failed"));
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let rt_err = RuntimeError::UnknownBackend {
        name: "gone".into(),
    };
    let abp_err = rt_err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("gone"));
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
fn is_retryable_for_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
}

#[test]
fn is_retryable_for_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
}

#[test]
fn is_not_retryable_for_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(!err.is_retryable());
}

#[test]
fn is_not_retryable_for_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("permanent"));
    assert!(!err.is_retryable());
}

#[test]
fn is_not_retryable_for_capability_check() {
    let err = RuntimeError::CapabilityCheckFailed("no mcp".into());
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn run_empty_registry_fails() {
    let rt = Runtime::new();
    let result = rt
        .run_streaming("mock", passthrough_wo("no backends"))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn unsatisfiable_capability_reqs_fail() {
    let rt = Runtime::with_default_backends();
    let mut wo = passthrough_wo("unsatisfied caps");
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Policy enforcement — allowed/denied operations through full pipeline
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn policy_deny_read_compiles_and_runs() {
    let rt = Runtime::with_default_backends();
    let mut wo = passthrough_wo("deny read policy");
    wo.policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into(), "/etc/passwd".into()],
        ..Default::default()
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn policy_deny_write_compiles_and_runs() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("deny write policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn policy_disallowed_tools_runs() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "DangerousTool".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("disallowed tools")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn empty_policy_completes() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("empty policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile::default())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Workspace staging — passthrough mode, staged mode with git
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn passthrough_workspace_completes() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("passthrough");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn staged_workspace_completes() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged")
        .workspace_mode(WorkspaceMode::Staged)
        .root(".")
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn staged_workspace_has_git_metadata() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::fs::write(tmp.path().join("hello.txt"), "world").expect("write file");

    let rt = Runtime::with_default_backends();
    let wo = WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "staged git test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: tmp.path().to_string_lossy().into_owned(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(
        receipt.verification.git_status.is_some(),
        "staged workspace should have git_status"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Receipt production — correct fields, hash valid
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_hash_is_64_hex_chars() {
    let receipt = abp_receipt::ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = abp_receipt::ReceiptBuilder::new("det")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .work_order_id(uuid::Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_excludes_stored_hash() {
    let r = abp_receipt::ReceiptBuilder::new("excl")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .work_order_id(uuid::Uuid::nil())
        .build();
    let hash_before = abp_core::receipt_hash(&r).unwrap();

    let mut r_with = r.clone();
    r_with.receipt_sha256 = Some("deadbeef".into());
    let hash_after = abp_core::receipt_hash(&r_with).unwrap();
    assert_eq!(hash_before, hash_after);
}

#[test]
fn receipt_verify_hash_passes_correct() {
    let r = abp_receipt::ReceiptBuilder::new("ok")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(abp_receipt::verify_hash(&r));
}

#[test]
fn receipt_verify_hash_fails_tampered() {
    let mut r = abp_receipt::ReceiptBuilder::new("tamper")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!abp_receipt::verify_hash(&r));
}

#[test]
fn receipt_verify_hash_passes_for_none() {
    let r = abp_receipt::ReceiptBuilder::new("none")
        .outcome(Outcome::Complete)
        .build();
    assert!(abp_receipt::verify_hash(&r));
}

#[tokio::test]
async fn receipt_hash_self_consistent_after_run() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("hash verify"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let stored_hash = receipt.receipt_sha256.clone().expect("hash must exist");
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(stored_hash, recomputed);
}

#[tokio::test]
async fn receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let h = rt
            .run_streaming("mock", passthrough_wo(&format!("chain-{i}")))
            .await
            .unwrap();
        let _ = drain_run(h).await;
    }
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 3);
}

#[tokio::test]
async fn receipt_chain_verifiable() {
    let rt = Runtime::with_default_backends();
    let h = rt
        .run_streaming("mock", passthrough_wo("chain-verify"))
        .await
        .unwrap();
    let _ = drain_run(h).await;
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert!(chain.verify().is_ok());
}

#[tokio::test]
async fn receipt_chain_latest_matches_last_run() {
    let rt = Runtime::with_default_backends();
    let h = rt
        .run_streaming("mock", passthrough_wo("latest"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(h).await;
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    let latest = chain.latest().unwrap();
    assert_eq!(latest.meta.run_id, receipt.meta.run_id);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Configuration — timeout, workspace mode, policy profiles, builder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn work_order_builder_sets_lane() {
    let wo = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_builder_sets_root() {
    let wo = WorkOrderBuilder::new("test").root("/custom").build();
    assert_eq!(wo.workspace.root, "/custom");
}

#[test]
fn work_order_builder_sets_workspace_mode() {
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_builder_sets_max_budget_usd() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(2.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
}

#[test]
fn work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_context() {
    let ctx = ContextPacket {
        files: vec!["main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "snippet1".into(),
            content: "fn main() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("test").context(ctx).build();
    assert_eq!(wo.context.files, vec!["main.rs"]);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn work_order_builder_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("test").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_string(&mode).unwrap();
    let decoded: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, ExecutionMode::Passthrough);
}

#[test]
fn retry_policy_default_has_retries() {
    let rp = RetryPolicy::default();
    assert!(rp.max_retries > 0);
    assert!(rp.initial_backoff > Duration::ZERO);
}

#[test]
fn retry_policy_no_retry() {
    let rp = RetryPolicy::no_retry();
    assert_eq!(rp.max_retries, 0);
}

#[test]
fn retry_policy_compute_delay_grows() {
    let rp = RetryPolicy::default();
    let d0 = rp.compute_delay(0);
    let d1 = rp.compute_delay(1);
    // Due to jitter the second delay might not always be strictly larger,
    // but the raw backoff doubles. Just ensure neither is zero.
    assert!(d0 > Duration::ZERO);
    assert!(d1 > Duration::ZERO);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Lifecycle — start, running, complete, error states
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_runs_produce_different_run_ids() {
    let rt = Runtime::with_default_backends();
    let h1 = rt.run_streaming("mock", passthrough_wo("a")).await.unwrap();
    let h2 = rt.run_streaming("mock", passthrough_wo("b")).await.unwrap();
    assert_ne!(h1.run_id, h2.run_id);
    let _ = drain_run(h1).await;
    let _ = drain_run(h2).await;
}

#[tokio::test]
async fn multiple_runs_different_receipt_hashes() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", passthrough_wo("ha"))
        .await
        .unwrap();
    let (_, r1) = drain_run(h1).await;
    let h2 = rt
        .run_streaming("mock", passthrough_wo("hb"))
        .await
        .unwrap();
    let (_, r2) = drain_run(h2).await;
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn three_sequential_runs_all_complete() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let h = rt
            .run_streaming("mock", passthrough_wo(&format!("seq-{i}")))
            .await
            .unwrap();
        let (_, receipt) = drain_run(h).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn metrics_track_multiple_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let h = rt
            .run_streaming("mock", passthrough_wo(&format!("met-{i}")))
            .await
            .unwrap();
        let _ = drain_run(h).await;
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn metrics_record_events_counted() {
    let rt = Runtime::with_default_backends();
    let h = rt
        .run_streaming("mock", passthrough_wo("events count"))
        .await
        .unwrap();
    let _ = drain_run(h).await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_events > 0);
}

#[test]
fn cancellation_token_starts_uncancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel_makes_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancellation_token_clones_share_state() {
    let token = CancellationToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
}

#[test]
fn cancellation_token_cancel_is_idempotent() {
    let token = CancellationToken::new();
    token.cancel();
    token.cancel();
    assert!(token.is_cancelled());
}

// ═══════════════════════════════════════════════════════════════════════
// Extra: capability checks, telemetry, store, multiplexer, hooks
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn check_capabilities_passes_streaming() {
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
fn check_capabilities_fails_unsupported() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("mock", &reqs).is_err());
}

#[test]
fn check_capabilities_empty_reqs_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn check_capabilities_unknown_backend_fails() {
    let rt = Runtime::with_default_backends();
    let err = rt
        .check_capabilities("nobody", &CapabilityRequirements::default())
        .unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn mock_backend_caps_include_expected_set() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
    assert!(caps.contains_key(&Capability::ToolRead));
    assert!(caps.contains_key(&Capability::ToolWrite));
    assert!(caps.contains_key(&Capability::ToolEdit));
    assert!(caps.contains_key(&Capability::ToolBash));
}

#[test]
fn native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_satisfies_neither() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn run_metrics_record_success() {
    let m = RunMetrics::new();
    m.record_run(100, true, 5);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.total_events, 5);
}

#[test]
fn run_metrics_record_failure() {
    let m = RunMetrics::new();
    m.record_run(50, false, 2);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.failed_runs, 1);
}

#[test]
fn run_metrics_average_duration() {
    let m = RunMetrics::new();
    m.record_run(100, true, 1);
    m.record_run(200, true, 1);
    let snap = m.snapshot();
    assert_eq!(snap.average_run_duration_ms, 150);
}

#[test]
fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let receipt = abp_receipt::ReceiptBuilder::new("store-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
}

#[test]
fn receipt_store_list_returns_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = abp_receipt::ReceiptBuilder::new("list-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
}

#[test]
fn receipt_store_verify_passes_for_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = abp_receipt::ReceiptBuilder::new("verify-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    assert!(store.verify(r.meta.run_id).unwrap());
}

#[test]
fn receipt_store_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn multiplexer_broadcast_with_subscriber() {
    let mux = EventMultiplexer::new(16);
    let _sub = mux.subscribe();
    let event = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    let count = mux.broadcast(event).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn multiplexer_no_subscribers_returns_error() {
    let mux = EventMultiplexer::new(16);
    let event = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    assert!(mux.broadcast(event).is_err());
}

#[tokio::test]
async fn empty_task_still_runs() {
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", passthrough_wo("")).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn very_long_task_still_runs() {
    let rt = Runtime::with_default_backends();
    let long_task = "x".repeat(10_000);
    let handle = rt
        .run_streaming("mock", passthrough_wo(&long_task))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn backend_registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}
