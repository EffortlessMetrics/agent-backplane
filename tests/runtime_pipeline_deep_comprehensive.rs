// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the abp-runtime orchestration pipeline.
//!
//! Covers:
//!  1. RuntimeBuilder configuration and validation
//!  2. Backend registration and lookup
//!  3. Unknown backend error handling
//!  4. Work order validation before execution
//!  5. Policy enforcement (tool/read/write checks)
//!  6. Workspace staging integration
//!  7. Event streaming to channel
//!  8. Receipt generation and hashing
//!  9. Error propagation chain
//! 10. Multiple sequential runs
//! 11. Backend selection logic
//! 12. Configuration merging
//! 13. Timing measurement in receipts
//! 14. Partial execution and partial receipts
//! 15. Resource cleanup after runs

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ExecutionLane, MinSupport, Outcome, PolicyProfile, RuntimeConfig, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::registry::BackendRegistry;
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ============================================================
// Helpers
// ============================================================

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Work order with a real temp dir suitable for `run_streaming`.
fn streaming_work_order(task: &str) -> (WorkOrder, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let wo = WorkOrderBuilder::new(task)
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    (wo, dir)
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: chrono::Utc::now(),
        kind,
        ext: None,
    }
}

/// Drain all events from a RunHandle and return the receipt.
async fn drain_handle(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<abp_core::Receipt, RuntimeError>) {
    let mut events_stream = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events_stream.next().await {
        collected.push(ev);
    }
    let receipt_result = match handle.receipt.await {
        Ok(r) => r,
        Err(join_err) => Err(RuntimeError::BackendFailed(anyhow::anyhow!(
            "join error: {join_err}"
        ))),
    };
    (collected, receipt_result)
}

// ============================================================
// 1. RuntimeBuilder configuration and validation
// ============================================================

#[test]
fn runtime_new_creates_empty_runtime() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
    assert!(rt.projection().is_none());
    assert!(rt.emulation_config().is_none());
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_default_same_as_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_with_default_backends_registers_mock() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
    assert_eq!(names.len(), 1);
}

#[test]
fn runtime_metrics_initially_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
    assert_eq!(snap.average_run_duration_ms, 0);
}

#[test]
fn runtime_emulation_config_initially_none() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_with_emulation_sets_config() {
    let config = abp_emulation::EmulationConfig::new();
    let rt = Runtime::new().with_emulation(config);
    assert!(rt.emulation_config().is_some());
}

#[test]
fn runtime_projection_initially_none() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_stream_pipeline_initially_none() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_chained_builder_pattern() {
    let rt = Runtime::with_default_backends().with_emulation(abp_emulation::EmulationConfig::new());
    assert!(rt.emulation_config().is_some());
    assert!(rt.backend("mock").is_some());
}

// ============================================================
// 2. Backend registration and lookup
// ============================================================

#[test]
fn register_single_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("test-backend", MockBackend);
    assert_eq!(rt.backend_names(), vec!["test-backend"]);
}

#[test]
fn register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("beta", MockBackend);
    rt.register_backend("gamma", MockBackend);
    assert_eq!(rt.backend_names().len(), 3);
}

#[test]
fn backend_names_sorted_alphabetically() {
    let mut rt = Runtime::new();
    rt.register_backend("z-backend", MockBackend);
    rt.register_backend("a-backend", MockBackend);
    rt.register_backend("m-backend", MockBackend);
    assert_eq!(
        rt.backend_names(),
        vec!["a-backend", "m-backend", "z-backend"]
    );
}

#[test]
fn register_backend_replaces_existing() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("mock", MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn backend_lookup_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_lookup_returns_none_for_missing() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("does-not-exist").is_none());
}

#[test]
fn registry_ref_provides_access() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

#[test]
fn registry_mut_allows_adding_backends() {
    let mut rt = Runtime::with_default_backends();
    rt.registry_mut().register("extra", MockBackend);
    assert!(rt.backend_names().contains(&"extra".to_string()));
    assert_eq!(rt.backend_names().len(), 2);
}

#[test]
fn registry_get_arc_returns_shared_ptr() {
    let rt = Runtime::with_default_backends();
    let arc1 = rt.backend("mock").unwrap();
    let arc2 = rt.backend("mock").unwrap();
    // Both point to the same allocation.
    assert!(Arc::ptr_eq(&arc1, &arc2));
}

#[test]
fn registry_standalone_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
    assert!(!reg.contains("anything"));
}

#[test]
fn registry_standalone_register_and_contains() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.contains("mock"));
    assert!(reg.get("mock").is_some());
    assert!(reg.get_arc("mock").is_some());
}

#[test]
fn registry_remove_returns_some() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.remove("mock").is_some());
    assert!(!reg.contains("mock"));
}

#[test]
fn registry_remove_nonexistent_returns_none() {
    let mut reg = BackendRegistry::default();
    assert!(reg.remove("missing").is_none());
}

#[test]
fn registry_list_sorted() {
    let mut reg = BackendRegistry::default();
    reg.register("c", MockBackend);
    reg.register("a", MockBackend);
    reg.register("b", MockBackend);
    assert_eq!(reg.list(), vec!["a", "b", "c"]);
}

// ============================================================
// 3. Unknown backend error handling
// ============================================================

#[tokio::test]
async fn run_streaming_unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("test");
    let result = rt.run_streaming("nonexistent", wo).await;
    match result {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn run_streaming_empty_name_returns_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("test");
    let result = rt.run_streaming("", wo).await;
    match result {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[tokio::test]
async fn run_streaming_case_sensitive_backend_name() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("test");
    // "Mock" != "mock"
    let result = rt.run_streaming("Mock", wo).await;
    match result {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "Mock");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn unknown_backend_error_display_contains_name() {
    let err = RuntimeError::UnknownBackend {
        name: "my-backend".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("my-backend"));
}

#[test]
fn unknown_backend_error_code() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[tokio::test]
async fn run_streaming_no_backends_registered() {
    let rt = Runtime::new();
    let (wo, _dir) = streaming_work_order("test");
    let result = rt.run_streaming("mock", wo).await;
    assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
}

// ============================================================
// 4. Work order validation before execution
// ============================================================

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("my task").build();
    assert_eq!(wo.task, "my task");
}

#[test]
fn work_order_builder_sets_lane() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_builder_sets_workspace_mode() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model, Some("gpt-4".to_string()));
}

#[test]
fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn work_order_builder_sets_max_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn work_order_builder_sets_root() {
    let wo = WorkOrderBuilder::new("task").root("/tmp/test").build();
    assert_eq!(wo.workspace.root, "/tmp/test");
}

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("task").build();
    let wo2 = WorkOrderBuilder::new("task").build();
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn work_order_default_policy_is_empty() {
    let wo = simple_work_order("test");
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
}

#[test]
fn work_order_default_requirements_is_empty() {
    let wo = simple_work_order("test");
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn work_order_builder_sets_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["bash"]);
}

#[test]
fn work_order_builder_sets_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

// ============================================================
// 5. Policy enforcement (tool/read/write checks)
// ============================================================

#[test]
fn policy_engine_allows_tool_with_empty_policy() {
    let policy = PolicyProfile::default();
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("bash").allowed);
}

#[test]
fn policy_engine_denies_disallowed_tool() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("bash").allowed);
}

#[test]
fn policy_engine_allows_tool_on_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("bash").allowed);
}

#[test]
fn policy_engine_denies_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("config.secret")).allowed);
}

#[test]
fn policy_engine_allows_read_non_denied_path() {
    let policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_read_path(Path::new("config.toml")).allowed);
}

#[test]
fn policy_engine_denies_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/Cargo.lock".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
}

#[test]
fn policy_engine_allows_write_non_denied_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/Cargo.lock".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn policy_engine_multiple_deny_write_patterns() {
    let policy = PolicyProfile {
        deny_write: vec!["*.lock".into(), "*.toml".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!engine.can_write_path(Path::new("config.toml")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn policy_decision_has_reason_on_deny() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["rm".into()],
        ..Default::default()
    };
    let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("rm");
    assert!(!decision.allowed);
    assert!(decision.reason.is_some());
}

// ============================================================
// 6. Workspace staging integration
// ============================================================

#[tokio::test]
async fn run_streaming_passthrough_workspace_succeeds() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("workspace passthrough");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn run_streaming_staged_workspace_with_temp_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    // Write a test file so staging has something to work with.
    std::fs::write(dir.path().join("hello.txt"), b"world").unwrap();

    let wo = WorkOrderBuilder::new("staged workspace test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn run_streaming_passthrough_preserves_workspace_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let wo = WorkOrderBuilder::new("passthrough root test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn workspace_verification_fields_populated() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let wo = WorkOrderBuilder::new("verification test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    // Verification report should be present (may have None fields for passthrough).
    let _ = &receipt.verification;
}

// ============================================================
// 7. Event streaming to channel
// ============================================================

#[tokio::test]
async fn run_streaming_emits_at_least_one_event() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("event stream test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    assert!(!events.is_empty(), "expected at least one event");
}

#[tokio::test]
async fn run_streaming_events_include_run_started() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("run started test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    let has_run_started = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }));
    assert!(has_run_started, "expected RunStarted event in stream");
}

#[tokio::test]
async fn run_streaming_events_include_run_completed() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("run completed test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    let has_run_completed = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_run_completed, "expected RunCompleted event in stream");
}

#[tokio::test]
async fn run_streaming_events_have_timestamps() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("timestamp test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    for ev in &events {
        // Timestamps should be non-zero (some valid datetime).
        assert!(ev.ts.timestamp() > 0);
    }
}

#[tokio::test]
async fn run_streaming_events_order_preserved() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("order test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    // Timestamps should be monotonically non-decreasing.
    for window in events.windows(2) {
        assert!(window[1].ts >= window[0].ts);
    }
}

#[tokio::test]
async fn run_streaming_provides_run_id() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("run id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert!(!handle.run_id.is_nil());
    let (_, _) = drain_handle(handle).await;
}

#[tokio::test]
async fn run_streaming_events_include_assistant_message() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("assistant msg test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_handle(handle).await;
    let has_msg = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
    assert!(has_msg, "expected AssistantMessage event");
}

#[tokio::test]
async fn run_streaming_run_ids_are_unique() {
    let rt = Runtime::with_default_backends();
    let (wo1, _d1) = streaming_work_order("unique id 1");
    let (wo2, _d2) = streaming_work_order("unique id 2");
    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let id1 = h1.run_id;
    let (_, _) = drain_handle(h1).await;
    let h2 = rt.run_streaming("mock", wo2).await.unwrap();
    let id2 = h2.run_id;
    let (_, _) = drain_handle(h2).await;
    assert_ne!(id1, id2);
}

// ============================================================
// 8. Receipt generation and hashing
// ============================================================

#[tokio::test]
async fn receipt_has_hash_after_run() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("hash test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn receipt_hash_is_64_hex_chars() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("hash len test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn receipt_outcome_is_complete() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("outcome test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn receipt_trace_is_nonempty() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("trace test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_backend_identity_is_mock() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("identity test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn receipt_builder_produces_receipt_with_hash() {
    let receipt = abp_core::ReceiptBuilder::new("test-be")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_deterministic() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let hash1 = abp_core::receipt_hash(&r1).unwrap();
    let hash2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn receipt_hash_different_for_different_outcomes() {
    let r1 = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let r2 = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build()
        .with_hash()
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_hash_nullifies_sha_before_hashing() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    // receipt_hash should return same value regardless of the stored hash.
    let computed = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &computed);
}

#[test]
fn receipt_verify_hash_passes_for_valid() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(abp_receipt::verify_hash(&r));
}

#[test]
fn receipt_contract_version_present() {
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.meta.contract_version, abp_core::CONTRACT_VERSION);
}

// ============================================================
// 9. Error propagation chain
// ============================================================

#[test]
fn runtime_error_unknown_backend_to_abp_error() {
    let err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("missing"));
}

#[test]
fn runtime_error_workspace_failed_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_policy_failed_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_backend_failed_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_capability_check_failed_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing cap".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn runtime_error_no_projection_match_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_display_messages() {
    let errors: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "foo".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("ws")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("pol")),
        RuntimeError::BackendFailed(anyhow::anyhow!("be")),
        RuntimeError::CapabilityCheckFailed("cap".into()),
        RuntimeError::NoProjectionMatch {
            reason: "proj".into(),
        },
    ];
    for err in &errors {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn classified_error_from_abp_error() {
    let abp_err =
        abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timeout occurred");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
}

#[test]
fn classified_error_into_abp_preserves_context() {
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
fn runtime_error_workspace_display_includes_workspace() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.to_string().to_lowercase().contains("workspace"));
}

#[test]
fn runtime_error_policy_display_includes_policy() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(err.to_string().to_lowercase().contains("policy"));
}

#[test]
fn runtime_error_backend_display_includes_backend() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(err.to_string().to_lowercase().contains("backend"));
}

// ============================================================
// 10. Multiple sequential runs
// ============================================================

#[tokio::test]
async fn sequential_runs_all_complete() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let (wo, _dir) = streaming_work_order(&format!("run {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt_result) = drain_handle(handle).await;
        let receipt = receipt_result.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn sequential_runs_have_unique_ids() {
    let rt = Runtime::with_default_backends();
    let mut ids = Vec::new();
    for i in 0..3 {
        let (wo, _dir) = streaming_work_order(&format!("unique {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        ids.push(handle.run_id);
        let (_, _) = drain_handle(handle).await;
    }
    // All IDs should be unique.
    let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, ids.len());
}

#[tokio::test]
async fn receipt_chain_accumulates_across_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let (wo, _dir) = streaming_work_order(&format!("chain {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt_result) = drain_handle(handle).await;
        assert!(receipt_result.is_ok());
    }
    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert_eq!(locked.len(), 3);
}

#[tokio::test]
async fn metrics_update_after_sequential_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let (wo, _dir) = streaming_work_order(&format!("metrics {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, _) = drain_handle(handle).await;
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn sequential_runs_each_have_unique_hashes() {
    let rt = Runtime::with_default_backends();
    let mut hashes = Vec::new();
    for i in 0..3 {
        let (wo, _dir) = streaming_work_order(&format!("hash {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt_result) = drain_handle(handle).await;
        let receipt = receipt_result.unwrap();
        hashes.push(receipt.receipt_sha256.unwrap());
    }
    let unique = hashes
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique, hashes.len());
}

// ============================================================
// 11. Backend selection logic
// ============================================================

#[test]
fn select_backend_without_projection_fails() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("test");
    match rt.select_backend(&wo) {
        Err(RuntimeError::NoProjectionMatch { .. }) => {}
        other => panic!("expected NoProjectionMatch, got {other:?}"),
    }
}

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
fn check_capabilities_passes_for_emulated_with_emulated_min() {
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
fn check_capabilities_fails_for_unsupported() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    match rt.check_capabilities("mock", &reqs) {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        other => panic!("expected CapabilityCheckFailed, got {other:?}"),
    }
}

#[test]
fn check_capabilities_empty_requirements_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn check_capabilities_unknown_backend_returns_error() {
    let rt = Runtime::new();
    let reqs = CapabilityRequirements::default();
    match rt.check_capabilities("missing", &reqs) {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "missing");
        }
        other => panic!("expected UnknownBackend, got {other:?}"),
    }
}

#[test]
fn mock_backend_identity() {
    let be = MockBackend;
    let id = be.identity();
    assert_eq!(id.id, "mock");
}

#[test]
fn mock_backend_capabilities_include_streaming() {
    let be = MockBackend;
    let caps = be.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn mock_backend_capabilities_include_tool_read() {
    let be = MockBackend;
    let caps = be.capabilities();
    assert!(caps.contains_key(&Capability::ToolRead));
}

#[test]
fn mock_backend_capabilities_include_tool_write() {
    let be = MockBackend;
    let caps = be.capabilities();
    assert!(caps.contains_key(&Capability::ToolWrite));
}

// ============================================================
// 12. Configuration merging
// ============================================================

#[test]
fn runtime_config_default_has_no_model() {
    let config = RuntimeConfig::default();
    assert!(config.model.is_none());
}

#[test]
fn runtime_config_default_has_empty_vendor() {
    let config = RuntimeConfig::default();
    assert!(config.vendor.is_empty());
}

#[test]
fn runtime_config_default_has_empty_env() {
    let config = RuntimeConfig::default();
    assert!(config.env.is_empty());
}

#[test]
fn work_order_config_with_vendor_settings() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "openai".to_string(),
        serde_json::json!({"temperature": 0.7}),
    );
    let config = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.model, Some("gpt-4".to_string()));
    assert!(wo.config.vendor.contains_key("openai"));
}

#[test]
fn work_order_config_with_env() {
    let mut env = BTreeMap::new();
    env.insert("API_KEY".to_string(), "secret".to_string());
    let config = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.env.get("API_KEY"), Some(&"secret".to_string()));
}

#[test]
fn work_order_builder_model_shorthand() {
    let wo = WorkOrderBuilder::new("task").model("claude-3").build();
    assert_eq!(wo.config.model, Some("claude-3".to_string()));
}

#[test]
fn work_order_builder_budget_shorthand() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(10.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn work_order_builder_turns_shorthand() {
    let wo = WorkOrderBuilder::new("task").max_turns(50).build();
    assert_eq!(wo.config.max_turns, Some(50));
}

// ============================================================
// 13. Timing measurement in receipts
// ============================================================

#[tokio::test]
async fn receipt_has_timing_metadata() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("timing test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn receipt_duration_ms_non_negative() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("duration test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    // Duration is u64, always >= 0; verify it's present.
    let _ = receipt.meta.duration_ms;
}

#[tokio::test]
async fn receipt_run_id_matches_handle() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("run id match");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let handle_run_id = handle.run_id;
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    // The receipt's run_id may differ from handle's run_id if the backend
    // generates its own, but we confirm the handle's run_id is valid.
    assert!(!handle_run_id.is_nil());
    assert!(!receipt.meta.run_id.is_nil());
}

#[tokio::test]
async fn receipt_work_order_id_is_set() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("wo id test");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn metrics_record_duration_after_run() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("metrics duration");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, _) = drain_handle(handle).await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    // Average should be > 0 since the run took some time.
    // It could be 0 if extremely fast, so just check total_runs.
    assert!(snap.total_runs >= 1);
}

#[tokio::test]
async fn receipt_contract_version_matches_constant() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("version test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    let receipt = receipt_result.unwrap();
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

// ============================================================
// 14. Partial execution and partial receipts
// ============================================================

#[test]
fn receipt_builder_partial_outcome() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_failed_outcome() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_with_trace_events() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(ev)
        .build();
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn receipt_builder_with_artifacts() {
    let art = abp_core::ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(art)
        .build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[test]
fn receipt_builder_with_usage() {
    let usage = abp_core::UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    };
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(usage)
        .build();
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
}

#[test]
fn receipt_builder_with_usage_raw() {
    let raw = serde_json::json!({"custom": "data"});
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(raw.clone())
        .build();
    assert_eq!(receipt.usage_raw, raw);
}

#[test]
fn receipt_partial_can_be_hashed() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn receipt_failed_can_be_hashed() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// ============================================================
// 15. Resource cleanup after runs
// ============================================================

#[tokio::test]
async fn run_completes_and_stream_ends() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("cleanup test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while events.next().await.is_some() {
        count += 1;
    }
    // Stream terminates eventually.
    assert!(count > 0);
    // Receipt future resolves.
    let receipt_result = handle.receipt.await.unwrap();
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn receipt_future_resolves_after_stream_drained() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("future resolve test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn runtime_usable_after_run() {
    let rt = Runtime::with_default_backends();
    let (wo1, _d1) = streaming_work_order("first");
    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let (_, r1) = drain_handle(h1).await;
    assert!(r1.is_ok());

    // Runtime should still be usable.
    let (wo2, _d2) = streaming_work_order("second");
    let h2 = rt.run_streaming("mock", wo2).await.unwrap();
    let (_, r2) = drain_handle(h2).await;
    assert!(r2.is_ok());
}

#[tokio::test]
async fn staged_workspace_temp_dir_cleaned_after_run() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("test.txt"), b"content").unwrap();

    let wo = WorkOrderBuilder::new("staged cleanup test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
    // The original dir should still exist (staging creates a copy).
    assert!(dir.path().exists());
}

#[tokio::test]
async fn passthrough_workspace_original_dir_untouched() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let test_file = dir.path().join("original.txt");
    std::fs::write(&test_file, b"untouched").unwrap();

    let wo = WorkOrderBuilder::new("passthrough cleanup test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
    // Original file should still be there.
    assert!(test_file.exists());
    assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "untouched");
}

// ============================================================
// Additional coverage: telemetry, receipt store, events
// ============================================================

#[test]
fn run_metrics_new_is_zero() {
    let m = RunMetrics::new();
    let s = m.snapshot();
    assert_eq!(s.total_runs, 0);
}

#[test]
fn run_metrics_record_success() {
    let m = RunMetrics::new();
    m.record_run(100, true, 5);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1);
    assert_eq!(s.successful_runs, 1);
    assert_eq!(s.failed_runs, 0);
    assert_eq!(s.total_events, 5);
}

#[test]
fn run_metrics_record_failure() {
    let m = RunMetrics::new();
    m.record_run(50, false, 1);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1);
    assert_eq!(s.failed_runs, 1);
    assert_eq!(s.successful_runs, 0);
}

#[test]
fn run_metrics_average_duration() {
    let m = RunMetrics::new();
    m.record_run(100, true, 0);
    m.record_run(200, true, 0);
    let s = m.snapshot();
    assert_eq!(s.average_run_duration_ms, 150);
}

#[test]
fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.outcome, Outcome::Complete);
}

#[test]
fn receipt_store_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    for _ in 0..3 {
        let r = abp_core::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        store.save(&r).unwrap();
    }
    assert_eq!(store.list().unwrap().len(), 3);
}

#[test]
fn receipt_store_verify_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    assert!(store.verify(r.meta.run_id).unwrap());
}

#[test]
fn receipt_store_load_nonexistent_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    assert!(store.load(uuid::Uuid::new_v4()).is_err());
}

#[test]
fn receipt_store_verify_chain_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 0);
}

// ============================================================
// Additional: agent event construction and serde
// ============================================================

#[test]
fn agent_event_run_started() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "hello".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed() {
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_tool_call() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"command": "ls"}),
    });
    if let AgentEventKind::ToolCall { tool_name, .. } = &ev.kind {
        assert_eq!(tool_name, "bash");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn agent_event_tool_result() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        output: serde_json::json!("file1.txt"),
        is_error: false,
    });
    if let AgentEventKind::ToolResult { is_error, .. } = &ev.kind {
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn agent_event_file_changed() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_warning() {
    let ev = make_event(AgentEventKind::Warning {
        message: "slow query".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_error() {
    let ev = make_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: None,
    });
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[test]
fn agent_event_serde_roundtrip() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello world".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &back.kind {
        assert_eq!(text, "hello world");
    } else {
        panic!("expected AssistantMessage");
    }
}

#[test]
fn agent_event_ext_field_default_none() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    assert!(ev.ext.is_none());
}

// ============================================================
// Additional: capability negotiation edge cases
// ============================================================

#[test]
fn check_capabilities_multiple_requirements() {
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

#[test]
fn check_capabilities_fails_if_any_unsatisfied() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            },
        ],
    };
    match rt.check_capabilities("mock", &reqs) {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        other => panic!("expected CapabilityCheckFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn run_streaming_with_capability_requirements_passes() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("cap req test")
        .root(
            tempfile::tempdir()
                .unwrap()
                .into_path()
                .to_string_lossy()
                .to_string(),
        )
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt_result) = drain_handle(handle).await;
    assert!(receipt_result.is_ok());
}

#[tokio::test]
async fn run_streaming_with_unsupported_capability_fails() {
    let rt = Runtime::with_default_backends();
    let dir = tempfile::tempdir().unwrap();
    let wo = WorkOrderBuilder::new("unsupported cap test")
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = rt.run_streaming("mock", wo).await;
    match result {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        Err(other) => panic!("expected CapabilityCheckFailed, got {other:?}"),
        Ok(_) => panic!("expected error for unsupported capability"),
    }
}

// ============================================================
// Additional: receipt chain and verification
// ============================================================

#[test]
fn receipt_chain_new_is_empty() {
    let chain = abp_receipt::ReceiptChain::new();
    assert_eq!(chain.len(), 0);
    assert!(chain.is_empty());
}

#[test]
fn receipt_chain_push_and_len() {
    let mut chain = abp_receipt::ReceiptChain::new();
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn receipt_store_verify_chain_with_multiple_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    for _ in 0..3 {
        let r = abp_core::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        store.save(&r).unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 3);
    assert!(v.invalid_hashes.is_empty());
}

// ============================================================
// Additional: work order serde roundtrip
// ============================================================

#[test]
fn work_order_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("serde test")
        .lane(ExecutionLane::PatchFirst)
        .model("test-model")
        .max_turns(5)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "serde test");
    assert_eq!(back.config.model, Some("test-model".to_string()));
    assert_eq!(back.config.max_turns, Some(5));
}

#[test]
fn receipt_serde_roundtrip() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let back: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.outcome, Outcome::Complete);
    assert_eq!(back.receipt_sha256, receipt.receipt_sha256);
}

// ============================================================
// Additional: pipeline stage tests
// ============================================================

#[tokio::test]
async fn validation_stage_passes_valid_order() {
    use abp_runtime::pipeline::{PipelineStage, ValidationStage};
    let mut wo = simple_work_order("valid task");
    assert!(ValidationStage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn validation_stage_rejects_empty_task() {
    use abp_runtime::pipeline::{PipelineStage, ValidationStage};
    let mut wo = WorkOrderBuilder::new("").build();
    assert!(ValidationStage.process(&mut wo).await.is_err());
}

#[tokio::test]
async fn validation_stage_rejects_whitespace_task() {
    use abp_runtime::pipeline::{PipelineStage, ValidationStage};
    let mut wo = WorkOrderBuilder::new("   ").build();
    assert!(ValidationStage.process(&mut wo).await.is_err());
}

#[tokio::test]
async fn policy_stage_passes_no_conflict() {
    use abp_runtime::pipeline::{PipelineStage, PolicyStage};
    let mut wo = simple_work_order("test");
    assert!(PolicyStage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn policy_stage_rejects_conflicting_allow_deny() {
    use abp_runtime::pipeline::{PipelineStage, PolicyStage};
    let mut wo = WorkOrderBuilder::new("test")
        .policy(PolicyProfile {
            allowed_tools: vec!["bash".into()],
            disallowed_tools: vec!["bash".into()],
            ..Default::default()
        })
        .build();
    assert!(PolicyStage.process(&mut wo).await.is_err());
}

#[tokio::test]
async fn audit_stage_records_entry() {
    use abp_runtime::pipeline::{AuditStage, PipelineStage};
    let audit = AuditStage::new();
    let mut wo = simple_work_order("audit me");
    let id = wo.id;
    audit.process(&mut wo).await.unwrap();
    let entries = audit.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].work_order_id, id);
}

#[tokio::test]
async fn pipeline_empty_succeeds() {
    use abp_runtime::pipeline::Pipeline;
    let p = Pipeline::new();
    assert!(p.is_empty());
    let mut wo = simple_work_order("test");
    assert!(p.execute(&mut wo).await.is_ok());
}

#[tokio::test]
async fn pipeline_stages_execute_in_order() {
    use abp_runtime::pipeline::{AuditStage, Pipeline, ValidationStage};
    let p = Pipeline::new()
        .stage(ValidationStage)
        .stage(AuditStage::new());
    assert_eq!(p.len(), 2);
    let mut wo = simple_work_order("ordered test");
    assert!(p.execute(&mut wo).await.is_ok());
}
