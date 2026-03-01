// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-crate integration tests verifying the full pipeline end-to-end.
//!
//! These tests exercise the chain:
//!   abp-core → abp-policy → abp-integrations → abp-runtime
//! to ensure WorkOrders flow correctly through the runtime, produce valid
//! receipts, and respect policy restrictions.

use std::path::Path;

use abp_core::validate::validate_receipt;
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Outcome, PolicyProfile, WorkOrderBuilder,
    WorkspaceMode,
};
use abp_integrations::MockBackend;
use abp_policy::PolicyEngine;
use abp_runtime::Runtime;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a work order to completion, collecting all streamed events and the receipt.
async fn run_to_completion(
    rt: &Runtime,
    backend: &str,
    wo: abp_core::WorkOrder,
) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let handle = rt
        .run_streaming(backend, wo)
        .await
        .expect("run_streaming should succeed");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle
        .receipt
        .await
        .expect("join handle should not panic")
        .expect("receipt should be Ok");
    (events, receipt)
}

// ===========================================================================
// 1. Full happy-path pipeline
// ===========================================================================

#[tokio::test]
async fn builder_to_runtime_to_receipt_happy_path() {
    // Build a WorkOrder using the builder API
    let wo = WorkOrderBuilder::new("integration test task")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("test-model")
        .max_turns(5)
        .max_budget_usd(1.0)
        .build();

    let wo_id = wo.id;

    // Run through Runtime with mock backend
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_to_completion(&rt, "mock", wo).await;

    // Receipt metadata
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(matches!(receipt.outcome, Outcome::Complete));

    // Receipt hash is present and self-consistent
    let hash = receipt.receipt_sha256.as_ref().expect("hash must be set");
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(hash, &recomputed, "stored hash must match recomputed hash");

    // Full receipt validation passes
    validate_receipt(&receipt).expect("receipt should pass validation");

    // Event stream includes bookend events
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. })),
        "events must include RunStarted"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. })),
        "events must include RunCompleted"
    );
}

#[tokio::test]
async fn receipt_trace_matches_streamed_events() {
    let wo = WorkOrderBuilder::new("trace consistency check")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_to_completion(&rt, "mock", wo).await;

    // The receipt trace should contain at least as many events as we streamed.
    assert!(!receipt.trace.is_empty(), "receipt trace must not be empty");
    assert_eq!(
        events.len(),
        receipt.trace.len(),
        "streamed events count must equal receipt trace count"
    );
}

#[tokio::test]
async fn receipt_timing_is_sane() {
    let wo = WorkOrderBuilder::new("timing check")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, "mock", wo).await;

    assert!(
        receipt.meta.started_at <= receipt.meta.finished_at,
        "started_at must be <= finished_at"
    );
}

// ===========================================================================
// 2. Policy enforcement
// ===========================================================================

#[tokio::test]
async fn policy_engine_enforces_tool_restrictions() {
    // Verify the PolicyEngine correctly denies disallowed tools
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/*.secret".into()],
        deny_write: vec!["**/protected/**".into()],
        ..Default::default()
    };

    let engine = PolicyEngine::new(&policy).expect("policy should compile");

    // Tool restrictions
    assert!(
        engine.can_use_tool("Read").allowed,
        "Read should be allowed"
    );
    assert!(
        !engine.can_use_tool("Bash").allowed,
        "Bash should be denied"
    );
    assert!(
        !engine.can_use_tool("Execute").allowed,
        "Execute not in allowlist"
    );

    // Path restrictions
    assert!(
        !engine.can_read_path(Path::new("config.secret")).allowed,
        "*.secret should be read-denied"
    );
    assert!(
        !engine
            .can_write_path(Path::new("protected/data.txt"))
            .allowed,
        "protected/ should be write-denied"
    );
    assert!(
        engine.can_read_path(Path::new("src/main.rs")).allowed,
        "normal files should be readable"
    );
    assert!(
        engine.can_write_path(Path::new("src/main.rs")).allowed,
        "normal files should be writable"
    );
}

#[tokio::test]
async fn restrictive_policy_still_completes_with_mock() {
    // A restrictive policy should compile and the mock backend should still
    // complete successfully (mock doesn't exercise tools gated by policy).
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_read: vec!["**/*.env".into(), "**/id_rsa".into()],
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };

    let wo = WorkOrderBuilder::new("policy enforcement test")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();

    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, "mock", wo).await;

    assert!(matches!(receipt.outcome, Outcome::Complete));
    validate_receipt(&receipt).expect("receipt should pass validation");
}

#[tokio::test]
async fn empty_policy_is_permissive() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).expect("compile empty policy");

    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("anything.txt")).allowed);
    assert!(engine.can_write_path(Path::new("anything.txt")).allowed);
}

// ===========================================================================
// 3. Multi-backend
// ===========================================================================

#[tokio::test]
async fn multiple_backends_produce_distinct_receipts() {
    let mut rt = Runtime::new();

    // Register the same MockBackend type under two different names.
    rt.register_backend("mock-alpha", MockBackend);
    rt.register_backend("mock-beta", MockBackend);

    assert!(rt.backend_names().contains(&"mock-alpha".into()));
    assert!(rt.backend_names().contains(&"mock-beta".into()));

    let wo_alpha = WorkOrderBuilder::new("alpha task")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let wo_beta = WorkOrderBuilder::new("beta task")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let (_ev_a, receipt_a) = run_to_completion(&rt, "mock-alpha", wo_alpha).await;
    let (_ev_b, receipt_b) = run_to_completion(&rt, "mock-beta", wo_beta).await;

    // Each run produces a distinct receipt (different run_ids, work_order_ids, hashes)
    assert_ne!(
        receipt_a.meta.run_id, receipt_b.meta.run_id,
        "run_ids must differ"
    );
    assert_ne!(
        receipt_a.meta.work_order_id, receipt_b.meta.work_order_id,
        "work_order_ids must differ"
    );
    assert_ne!(
        receipt_a.receipt_sha256, receipt_b.receipt_sha256,
        "receipt hashes must differ"
    );

    // Both receipts are independently valid
    validate_receipt(&receipt_a).expect("receipt_a should be valid");
    validate_receipt(&receipt_b).expect("receipt_b should be valid");
}

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("should fail")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err(), "unknown backend should return error");
    let msg = format!("{}", result.err().expect("should be Err"));
    assert!(
        msg.contains("nonexistent"),
        "error should mention backend name: {msg}"
    );
}

#[tokio::test]
async fn sequential_runs_on_same_backend_get_unique_ids() {
    let rt = Runtime::with_default_backends();
    let mut run_ids = std::collections::HashSet::new();
    let mut hashes = std::collections::HashSet::new();

    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("run {i}"))
            .root(".")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let (_events, receipt) = run_to_completion(&rt, "mock", wo).await;
        run_ids.insert(receipt.meta.run_id);
        if let Some(h) = &receipt.receipt_sha256 {
            hashes.insert(h.clone());
        }
        validate_receipt(&receipt).expect("receipt should be valid");
    }

    assert_eq!(run_ids.len(), 3, "each run must have a unique run_id");
    assert_eq!(hashes.len(), 3, "each receipt should have a unique hash");
}

#[tokio::test]
async fn backend_identity_is_populated() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("identity check")
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let (_events, receipt) = run_to_completion(&rt, "mock", wo).await;

    // The mock backend reports "mock" as its identity
    assert_eq!(receipt.backend.id, "mock");
    assert!(
        receipt.backend.backend_version.is_some(),
        "backend_version should be set"
    );

    // Capabilities manifest should be non-empty for mock
    assert!(
        !receipt.capabilities.is_empty(),
        "capabilities should be populated"
    );
}
