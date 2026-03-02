// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the runtime orchestration layer (80+ tests).

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ── Helpers ────────────────────────────────────────────────────────────

/// Minimal work order with PassThrough workspace (avoids staging overhead).
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

/// Drain the event stream and return collected events + the receipt.
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Runtime construction and configuration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_new_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_default_is_same_as_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_with_default_backends_includes_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn runtime_with_default_backends_has_exactly_one_backend() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn runtime_new_has_no_emulation() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_new_has_no_projection() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_new_has_no_stream_pipeline() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_with_emulation_stores_config() {
    let config = abp_emulation::EmulationConfig::new();
    let rt = Runtime::new().with_emulation(config.clone());
    assert!(rt.emulation_config().is_some());
}

#[test]
fn runtime_with_projection_stores_matrix() {
    let matrix = abp_projection::ProjectionMatrix::new();
    let rt = Runtime::new().with_projection(matrix);
    assert!(rt.projection().is_some());
}

#[test]
fn runtime_metrics_starts_at_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backend registration and lookup
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn register_backend_makes_it_discoverable() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("a", abp_integrations::MockBackend);
    rt.register_backend("b", abp_integrations::MockBackend);
    rt.register_backend("c", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names().len(), 3);
}

#[test]
fn backend_names_are_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zulu", abp_integrations::MockBackend);
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("mike", abp_integrations::MockBackend);
    let names = rt.backend_names();
    assert_eq!(names, vec!["alpha", "mike", "zulu"]);
}

#[test]
fn backend_returns_none_for_unregistered() {
    let rt = Runtime::new();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_identity_is_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    assert_eq!(b.identity().id, "mock");
}

#[test]
fn register_replaces_existing_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn registry_ref_contains_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("missing"));
}

#[test]
fn registry_mut_allows_removal() {
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
    let arc = rt.registry().get_arc("mock");
    assert!(arc.is_some());
}

#[test]
fn registry_list_matches_backend_names() {
    let rt = Runtime::with_default_backends();
    let list: Vec<String> = rt.registry().list().iter().map(|s| s.to_string()).collect();
    assert_eq!(list, rt.backend_names());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. WorkOrder preparation and builder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn work_order_builder_default_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_builder_default_workspace_mode_is_staged() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
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
fn work_order_builder_sets_include_exclude() {
    let wo = WorkOrderBuilder::new("test")
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn work_order_builder_sets_context() {
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
fn work_order_builder_sets_policy() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("test").policy(policy).build();
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
}

#[test]
fn work_order_builder_sets_requirements() {
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
fn work_order_builder_sets_config() {
    let config = RuntimeConfig {
        model: Some("claude-4".into()),
        max_turns: Some(20),
        ..RuntimeConfig::default()
    };
    let wo = WorkOrderBuilder::new("test").config(config).build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-4"));
    assert_eq!(wo.config.max_turns, Some(20));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Mock backend integration via runtime
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_streaming_mock_produces_events() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("emit events");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    assert!(!events.is_empty(), "mock backend should emit events");
}

#[tokio::test]
async fn run_streaming_mock_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("hash check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn run_streaming_mock_receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("version check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn run_streaming_mock_receipt_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("outcome check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_mock_backend_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("backend id");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn run_streaming_mock_includes_run_started() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("events check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let has_run_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    assert!(has_run_started, "should include RunStarted event");
}

#[tokio::test]
async fn run_streaming_mock_includes_run_completed() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("events check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let has_run_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_run_completed, "should include RunCompleted event");
}

#[tokio::test]
async fn run_streaming_mock_receipt_trace_is_populated() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("trace check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(!receipt.trace.is_empty(), "receipt trace should not be empty");
}

#[tokio::test]
async fn run_streaming_mock_receipt_verification_harness_ok() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("harness");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn run_streaming_run_id_is_nonzero() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("run id");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert_ne!(handle.run_id, uuid::Uuid::nil());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error handling
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_streaming_unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("unknown");
    let result = rt.run_streaming("nonexistent", wo).await;
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
fn unknown_backend_error_code() {
    let err = RuntimeError::UnknownBackend {
        name: "x".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn workspace_failed_error_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn policy_failed_error_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn backend_failed_error_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn capability_check_failed_error_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn no_projection_match_error_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "empty".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn classified_error_preserves_code() {
    let abp_err =
        abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
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
fn classified_error_roundtrip_preserves_context() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::ConfigInvalid, "bad")
        .with_context("key", "value");
    let rt_err: RuntimeError = abp_err.into();
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, abp_error::ErrorCode::ConfigInvalid);
    assert_eq!(
        back.context.get("key"),
        Some(&serde_json::json!("value"))
    );
}

#[test]
fn workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("io error"));
    assert!(err.to_string().contains("workspace preparation failed"));
}

#[test]
fn policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("glob error"));
    assert!(err.to_string().contains("policy compilation failed"));
}

#[test]
fn backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("panic"));
    assert!(err.to_string().contains("backend execution failed"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Capability checking
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn check_capabilities_passes_for_native_streaming() {
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
fn check_capabilities_passes_for_emulated_tool_read() {
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
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[test]
fn check_capabilities_empty_requirements_passes() {
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
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Receipt finalization and hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_hash_produces_64_char_hex() {
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
    let r1 = abp_receipt::ReceiptBuilder::new("det")
        .outcome(Outcome::Complete)
        .run_id(uuid::Uuid::nil())
        .work_order_id(uuid::Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r1).unwrap();
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
    assert_eq!(hash_before, hash_after, "stored hash must not affect computed hash");
}

#[test]
fn receipt_verify_hash_passes_for_correct_hash() {
    let r = abp_receipt::ReceiptBuilder::new("ok")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(abp_receipt::verify_hash(&r));
}

#[test]
fn receipt_verify_hash_fails_for_tampered() {
    let mut r = abp_receipt::ReceiptBuilder::new("tamper")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.receipt_sha256 = Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!abp_receipt::verify_hash(&r));
}

#[test]
fn receipt_verify_hash_passes_for_none() {
    let r = abp_receipt::ReceiptBuilder::new("none")
        .outcome(Outcome::Complete)
        .build();
    assert!(abp_receipt::verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Receipt chain
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn receipt_chain_accumulates_across_runs() {
    let rt = Runtime::with_default_backends();

    let h1 = rt.run_streaming("mock", passthrough_wo("run-1")).await.unwrap();
    let (_, _r1) = drain_run(h1).await;

    let h2 = rt.run_streaming("mock", passthrough_wo("run-2")).await.unwrap();
    let (_, _r2) = drain_run(h2).await;

    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 2);
}

#[tokio::test]
async fn receipt_chain_is_verifiable() {
    let rt = Runtime::with_default_backends();
    let h = rt.run_streaming("mock", passthrough_wo("chain-verify")).await.unwrap();
    let (_, _) = drain_run(h).await;

    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert!(chain.verify().is_ok());
}

#[tokio::test]
async fn receipt_chain_latest_returns_last() {
    let rt = Runtime::with_default_backends();
    let h = rt.run_streaming("mock", passthrough_wo("latest")).await.unwrap();
    let (_, receipt) = drain_run(h).await;

    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    let latest = chain.latest().unwrap();
    assert_eq!(latest.meta.run_id, receipt.meta.run_id);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Multiple sequential runs
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_runs_produce_different_run_ids() {
    let rt = Runtime::with_default_backends();
    let h1 = rt.run_streaming("mock", passthrough_wo("run-a")).await.unwrap();
    let h2 = rt.run_streaming("mock", passthrough_wo("run-b")).await.unwrap();
    assert_ne!(h1.run_id, h2.run_id);
    let _ = drain_run(h1).await;
    let _ = drain_run(h2).await;
}

#[tokio::test]
async fn multiple_runs_produce_different_receipt_hashes() {
    let rt = Runtime::with_default_backends();
    let h1 = rt.run_streaming("mock", passthrough_wo("hash-a")).await.unwrap();
    let (_, r1) = drain_run(h1).await;
    let h2 = rt.run_streaming("mock", passthrough_wo("hash-b")).await.unwrap();
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
            .run_streaming("mock", passthrough_wo(&format!("metrics-{i}")))
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

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn empty_task_still_runs() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn very_long_task_still_runs() {
    let rt = Runtime::with_default_backends();
    let long_task = "x".repeat(10_000);
    let wo = passthrough_wo(&long_task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_empty_registry_fails() {
    let rt = Runtime::new();
    let wo = passthrough_wo("no backends");
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err());
}

#[test]
fn backend_registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn backend_capabilities_are_accessible() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
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

// ═══════════════════════════════════════════════════════════════════════
// 11. Telemetry / RunMetrics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_metrics_new_all_zero() {
    let m = RunMetrics::new();
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
    assert_eq!(snap.average_run_duration_ms, 0);
}

#[test]
fn run_metrics_record_success() {
    let m = RunMetrics::new();
    m.record_run(100, true, 5);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 5);
    assert_eq!(snap.average_run_duration_ms, 100);
}

#[test]
fn run_metrics_record_failure() {
    let m = RunMetrics::new();
    m.record_run(50, false, 2);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 2);
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
fn run_metrics_accumulates_events() {
    let m = RunMetrics::new();
    m.record_run(10, true, 3);
    m.record_run(10, true, 7);
    let snap = m.snapshot();
    assert_eq!(snap.total_events, 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Receipt store
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let receipt = abp_receipt::ReceiptBuilder::new("store-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.backend.id, "store-test");
}

#[test]
fn receipt_store_list_returns_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let r = abp_receipt::ReceiptBuilder::new("list-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], r.meta.run_id);
}

#[test]
fn receipt_store_verify_passes_for_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
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
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let ids = store.list().unwrap();
    assert!(ids.is_empty());
}

#[test]
fn receipt_store_verify_chain_empty_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = abp_runtime::store::ReceiptStore::new(dir.path());
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Policy application
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_with_policy_disallowed_tools_still_completes() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "policy test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_with_deny_write_policy_still_completes() {
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
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_with_empty_policy_completes() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("empty policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile::default())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. SupportLevel / MinSupport (contract types used in runtime)
// ═══════════════════════════════════════════════════════════════════════

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
fn emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn unsupported_satisfies_neither() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_satisfies_emulated_but_not_native() {
    let r = SupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!r.satisfies(&MinSupport::Native));
    assert!(r.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Execution mode defaults
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// 16. WorkspaceMode / Staging
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_with_staged_workspace_completes() {
    let rt = Runtime::with_default_backends();
    // Use "." as root — staged mode copies the workspace into a temp dir.
    let wo = WorkOrderBuilder::new("staged test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(".")
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_with_passthrough_workspace_completes() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("passthrough test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Emulation integration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn emulation_config_roundtrip() {
    let mut config = abp_emulation::EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        abp_emulation::EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );
    let rt = Runtime::new().with_emulation(config.clone());
    let stored = rt.emulation_config().unwrap();
    assert_eq!(stored.strategies.len(), config.strategies.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Budget module smoke tests (via runtime)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn budget_tracker_within_limits() {
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        ..BudgetLimit::default()
    });
    t.record_tokens(500);
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

#[test]
fn budget_tracker_exceeded() {
    use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker};
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..BudgetLimit::default()
    });
    t.record_tokens(101);
    assert!(matches!(t.check(), BudgetStatus::Exceeded(_)));
}

#[test]
fn budget_remaining_tokens() {
    use abp_runtime::budget::{BudgetLimit, BudgetTracker};
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        ..BudgetLimit::default()
    });
    t.record_tokens(300);
    let rem = t.remaining();
    assert_eq!(rem.tokens, Some(700));
}
