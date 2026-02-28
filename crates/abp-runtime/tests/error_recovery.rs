// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error recovery tests for the ABP runtime.
//!
//! Verifies that the runtime remains usable after encountering errors:
//! registry stays consistent, metrics are accurate, and subsequent runs succeed.

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ExecutionLane, MinSupport,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::{Runtime, RuntimeError};
use std::error::Error;
use tokio_stream::StreamExt;

/// Extract the error from a `run_streaming` result (RunHandle lacks Debug).
async fn expect_run_err(
    result: Result<abp_runtime::RunHandle, RuntimeError>,
) -> RuntimeError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("expected an error from run_streaming"),
    }
}

/// Build a minimal work order suitable for the mock backend.
fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "error recovery test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: abp_core::PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

/// Build a work order with unsatisfiable capability requirements.
fn unsatisfiable_work_order() -> WorkOrder {
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    wo
}

/// Run a work order to completion, draining events and awaiting the receipt.
async fn run_to_completion(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> Result<abp_core::Receipt, String> {
    let handle = rt
        .run_streaming(backend, wo)
        .await
        .map_err(|e| e.to_string())?;
    let _: Vec<_> = handle.events.collect().await;
    handle
        .receipt
        .await
        .expect("join handle")
        .map_err(|e| e.to_string())
}

// ---------- 1. Unknown backend recovery ----------

#[tokio::test]
async fn unknown_backend_then_valid_backend_succeeds() {
    let rt = Runtime::with_default_backends();

    // First run: unknown backend should fail.
    let err = expect_run_err(rt.run_streaming("nonexistent", mock_work_order()).await).await;
    assert!(err.to_string().contains("nonexistent"));

    // Second run: valid backend should succeed without issue.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("valid run after unknown backend error");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 2. Capability check recovery ----------

#[tokio::test]
async fn capability_check_failure_then_success() {
    let rt = Runtime::with_default_backends();

    // First run: unsatisfiable requirements.
    let err = expect_run_err(rt.run_streaming("mock", unsatisfiable_work_order()).await).await;
    assert!(
        err.to_string().contains("capability"),
        "expected capability error, got: {err}"
    );

    // Second run: no requirements — should succeed.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("valid run after capability failure");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 3. Sequential error-success pattern ----------

#[tokio::test]
async fn alternating_error_success_pattern() {
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        // Failing run (unknown backend).
        let err = expect_run_err(rt.run_streaming("bad", mock_work_order()).await).await;
        assert!(
            err.to_string().contains("bad"),
            "iteration {i}: expected unknown backend error"
        );

        // Succeeding run.
        let receipt = run_to_completion(&rt, "mock", mock_work_order())
            .await
            .unwrap_or_else(|e| panic!("iteration {i}: expected success, got: {e}"));
        assert!(receipt.receipt_sha256.is_some());
    }
}

// ---------- 4. Error doesn't corrupt state ----------

#[tokio::test]
async fn error_does_not_corrupt_registry_or_metrics() {
    let rt = Runtime::with_default_backends();

    let names_before = rt.backend_names();
    let snap_before = rt.metrics().snapshot();

    // Trigger several errors.
    let _ = rt.run_streaming("nope", mock_work_order()).await;
    let _ = rt
        .run_streaming("mock", unsatisfiable_work_order())
        .await;

    // Registry must be unchanged.
    assert_eq!(
        rt.backend_names(),
        names_before,
        "registry must not change after errors"
    );

    // Metrics: errors that reject before spawning a run should not bump
    // total_runs (only the in-task code records metrics).
    let snap_after = rt.metrics().snapshot();
    assert_eq!(
        snap_before.total_runs, snap_after.total_runs,
        "pre-flight errors should not affect run count"
    );

    // A successful run should still work and update metrics.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("run after errors");
    assert!(receipt.receipt_sha256.is_some());

    let snap_final = rt.metrics().snapshot();
    assert_eq!(
        snap_final.total_runs,
        snap_before.total_runs + 1,
        "successful run should increment total_runs"
    );
    assert!(
        snap_final.successful_runs >= 1,
        "should have at least one successful run"
    );
}

// ---------- 5. Multiple different errors ----------

#[tokio::test]
async fn multiple_different_error_types_are_distinct() {
    let rt = Runtime::with_default_backends();

    // UnknownBackend
    let e1 = expect_run_err(rt.run_streaming("missing", mock_work_order()).await).await;

    // CapabilityCheckFailed
    let e2 = expect_run_err(rt.run_streaming("mock", unsatisfiable_work_order()).await).await;

    let msg1 = e1.to_string();
    let msg2 = e2.to_string();

    assert_ne!(msg1, msg2, "different error types should produce different messages");
    assert!(msg1.contains("unknown backend"), "e1: {msg1}");
    assert!(msg2.contains("capability"), "e2: {msg2}");
}

// ---------- 6. RuntimeError display coverage ----------

#[test]
fn runtime_error_display_coverage() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend {
            name: "test-backend".into(),
        },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob")),
        RuntimeError::BackendFailed(anyhow::anyhow!("timeout")),
        RuntimeError::CapabilityCheckFailed("missing streaming".into()),
    ];

    for variant in &variants {
        let display = variant.to_string();
        assert!(
            !display.is_empty(),
            "Display must be non-empty for {variant:?}"
        );
    }

    // Spot-check specific messages.
    assert!(variants[0].to_string().contains("test-backend"));
    assert!(variants[4].to_string().contains("missing streaming"));
}

// ---------- 7. RuntimeError Debug coverage ----------

#[test]
fn runtime_error_debug_coverage() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend {
            name: "dbg".into(),
        },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("ws")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("pol")),
        RuntimeError::BackendFailed(anyhow::anyhow!("be")),
        RuntimeError::CapabilityCheckFailed("cap".into()),
    ];

    for variant in &variants {
        let debug = format!("{variant:?}");
        assert!(
            !debug.is_empty(),
            "Debug must produce output for every variant"
        );
        // Debug output should contain the variant name.
        let contains_variant = debug.contains("UnknownBackend")
            || debug.contains("WorkspaceFailed")
            || debug.contains("PolicyFailed")
            || debug.contains("BackendFailed")
            || debug.contains("CapabilityCheckFailed");
        assert!(contains_variant, "Debug output should name the variant: {debug}");
    }
}

// ---------- 8. Error source chain ----------

#[test]
fn error_source_chain() {
    // Variants with #[source]: WorkspaceFailed, PolicyFailed, BackendFailed.
    let inner = anyhow::anyhow!("root cause");
    let err = RuntimeError::WorkspaceFailed(inner);
    assert!(
        err.source().is_some(),
        "WorkspaceFailed should have a source"
    );

    let inner = anyhow::anyhow!("policy root");
    let err = RuntimeError::PolicyFailed(inner);
    assert!(err.source().is_some(), "PolicyFailed should have a source");

    let inner = anyhow::anyhow!("backend root");
    let err = RuntimeError::BackendFailed(inner);
    assert!(
        err.source().is_some(),
        "BackendFailed should have a source"
    );

    // Variants without #[source]: UnknownBackend, CapabilityCheckFailed.
    let err = RuntimeError::UnknownBackend {
        name: "x".into(),
    };
    assert!(
        err.source().is_none(),
        "UnknownBackend should not have a source"
    );

    let err = RuntimeError::CapabilityCheckFailed("y".into());
    assert!(
        err.source().is_none(),
        "CapabilityCheckFailed should not have a source"
    );
}
