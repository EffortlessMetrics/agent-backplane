// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error recovery tests for the ABP runtime.
//!
//! Verifies that the runtime remains usable after encountering errors:
//! registry stays consistent, metrics are accurate, and subsequent runs succeed.
//! Also tests custom failing/panicking backends, empty streams, backpressure,
//! timeout, workspace failures, and invalid work order fields.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionLane, ExecutionMode, MinSupport,
    Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::error::Error;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

/// Extract the error from a `run_streaming` result (RunHandle lacks Debug).
async fn expect_run_err(result: Result<abp_runtime::RunHandle, RuntimeError>) -> RuntimeError {
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
    let _ = rt.run_streaming("mock", unsatisfiable_work_order()).await;

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

    assert_ne!(
        msg1, msg2,
        "different error types should produce different messages"
    );
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
        RuntimeError::UnknownBackend { name: "dbg".into() },
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
        assert!(
            contains_variant,
            "Debug output should name the variant: {debug}"
        );
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
    assert!(err.source().is_some(), "BackendFailed should have a source");

    // Variants without #[source]: UnknownBackend, CapabilityCheckFailed.
    let err = RuntimeError::UnknownBackend { name: "x".into() };
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

// =========================================================================
// Custom test backends
// =========================================================================

fn test_identity(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.to_string(),
        backend_version: Some("test".to_string()),
        adapter_version: None,
    }
}

fn test_receipt(run_id: Uuid, work_order_id: Uuid, backend_id: &str) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 0,
        },
        backend: test_identity(backend_id),
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

/// Backend that returns an error from `run`.
#[derive(Debug, Clone)]
struct ErrorBackend;

#[async_trait]
impl Backend for ErrorBackend {
    fn identity(&self) -> BackendIdentity {
        test_identity("error-backend")
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        anyhow::bail!("deliberate backend error")
    }
}

/// Backend that panics inside `run`.
#[derive(Debug, Clone)]
struct PanicBackend;

#[async_trait]
impl Backend for PanicBackend {
    fn identity(&self) -> BackendIdentity {
        test_identity("panic-backend")
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        panic!("deliberate backend panic")
    }
}

/// Backend that returns a receipt without emitting any events.
#[derive(Debug, Clone)]
struct EmptyStreamBackend;

#[async_trait]
impl Backend for EmptyStreamBackend {
    fn identity(&self) -> BackendIdentity {
        test_identity("empty-stream")
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        Ok(test_receipt(run_id, work_order.id, "empty-stream"))
    }
}

/// Backend that floods the event channel then returns a receipt.
#[derive(Debug, Clone)]
struct FloodBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for FloodBackend {
    fn identity(&self) -> BackendIdentity {
        test_identity("flood-backend")
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                },
                ext: None,
            };
            // Best-effort send; if channel is full, try_send will fail.
            if events_tx.send(ev).await.is_err() {
                break;
            }
        }
        Ok(test_receipt(run_id, work_order.id, "flood-backend"))
    }
}

/// Backend that sleeps longer than the test timeout.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay: std::time::Duration,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        test_identity("slow-backend")
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "slow start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        tokio::time::sleep(self.delay).await;
        Ok(test_receipt(run_id, work_order.id, "slow-backend"))
    }
}

// ---------- 9. Backend returns error — runtime handles gracefully ----------

#[tokio::test]
async fn backend_error_produces_runtime_error() {
    let mut rt = Runtime::new();
    rt.register_backend("error", ErrorBackend);

    let handle = rt
        .run_streaming("error", mock_work_order())
        .await
        .expect("run_streaming should start");

    let _events: Vec<_> = handle.events.collect().await;
    let result = handle.receipt.await.expect("join handle");

    assert!(result.is_err(), "backend error should propagate");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed, got: {err:?}"
    );
    assert!(
        err.to_string().contains("backend execution failed"),
        "error message: {}",
        err
    );
}

// ---------- 10. Backend panics — runtime doesn't crash ----------

#[tokio::test]
async fn backend_panic_does_not_crash_runtime() {
    let mut rt = Runtime::new();
    rt.register_backend("panic", PanicBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);

    let handle = rt
        .run_streaming("panic", mock_work_order())
        .await
        .expect("run_streaming should start");

    let _events: Vec<_> = handle.events.collect().await;
    let result = handle.receipt.await.expect("join handle");

    assert!(result.is_err(), "panic should produce an error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed from panic, got: {err:?}"
    );

    // Runtime should still be usable after the panic.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("runtime should recover after backend panic");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 11. Empty event stream — receipt still generated ----------

#[tokio::test]
async fn empty_event_stream_still_produces_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("empty", EmptyStreamBackend);

    let handle = rt
        .run_streaming("empty", mock_work_order())
        .await
        .expect("run_streaming should start");

    let events: Vec<_> = handle.events.collect().await;
    assert!(events.is_empty(), "no events should be emitted");

    let receipt = handle
        .receipt
        .await
        .expect("join handle")
        .expect("receipt should succeed");

    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt should have a hash"
    );
    assert!(
        matches!(receipt.outcome, Outcome::Complete),
        "empty stream should still complete: {:?}",
        receipt.outcome
    );
}

// ---------- 12. Channel backpressure — many events ----------

#[tokio::test]
async fn flood_backend_backpressure_handled() {
    let mut rt = Runtime::new();
    rt.register_backend("flood", FloodBackend { event_count: 1000 });

    let handle = rt
        .run_streaming("flood", mock_work_order())
        .await
        .expect("run_streaming should start");

    let events: Vec<_> = handle.events.collect().await;
    // We should receive events (the exact count depends on channel capacity
    // and timing, but should be > 0).
    assert!(!events.is_empty(), "should receive some events from flood");

    let receipt = handle
        .receipt
        .await
        .expect("join handle")
        .expect("receipt should succeed");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 13. Timeout simulation ----------

#[tokio::test]
async fn slow_backend_can_be_timed_out() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "slow",
        SlowBackend {
            delay: std::time::Duration::from_secs(30),
        },
    );

    let handle = rt
        .run_streaming("slow", mock_work_order())
        .await
        .expect("run_streaming should start");

    // Apply an external timeout shorter than the backend's delay.
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        let _events: Vec<_> = handle.events.collect().await;
        handle.receipt.await
    })
    .await;

    // The timeout should fire, proving the caller can cap slow backends.
    assert!(
        result.is_err(),
        "timeout should fire before slow backend completes"
    );
}

// ---------- 14. Multiple sequential failures then success ----------

#[tokio::test]
async fn multiple_sequential_failures_then_success() {
    let mut rt = Runtime::new();
    rt.register_backend("error", ErrorBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);

    // Run the error backend several times.
    for i in 0..5 {
        let handle = rt
            .run_streaming("error", mock_work_order())
            .await
            .expect("run_streaming should start");
        let _events: Vec<_> = handle.events.collect().await;
        let result = handle.receipt.await.expect("join handle");
        assert!(result.is_err(), "iteration {i}: should fail");
    }

    // Now run the mock backend — should succeed.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("should succeed after repeated failures");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 15. Workspace creation failure recovery ----------

#[tokio::test]
async fn workspace_staging_failure_recovery() {
    let rt = Runtime::with_default_backends();

    // Use a non-existent root with Staged mode to trigger workspace failure.
    let mut wo = mock_work_order();
    wo.workspace = WorkspaceSpec {
        root: "/nonexistent/path/that/does/not/exist/abp_test_xyz".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };

    let handle = rt.run_streaming("mock", wo).await;
    match handle {
        Err(RuntimeError::WorkspaceFailed(_)) => {
            // Pre-flight workspace failure — acceptable.
        }
        Ok(h) => {
            // The workspace failure happens inside the spawned task.
            let _events: Vec<_> = h.events.collect().await;
            let result = h.receipt.await.expect("join handle");
            assert!(
                result.is_err(),
                "workspace failure should produce an error in receipt"
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, RuntimeError::WorkspaceFailed(_)),
                "expected WorkspaceFailed, got: {err:?}"
            );
        }
        Err(other) => {
            panic!("expected WorkspaceFailed, got: {other:?}");
        }
    }

    // Runtime should still work after workspace failure.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("should succeed after workspace failure");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 16. Invalid work order — empty task ----------

#[tokio::test]
async fn empty_task_still_runs() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.task = String::new();

    // The runtime doesn't validate task content — the mock backend should still work.
    let receipt = run_to_completion(&rt, "mock", wo)
        .await
        .expect("empty task should not prevent execution");
    assert!(receipt.receipt_sha256.is_some());
}

// ---------- 17. Backend error then state check ----------

#[tokio::test]
async fn backend_error_does_not_corrupt_metrics() {
    let mut rt = Runtime::new();
    rt.register_backend("error", ErrorBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);

    let snap_before = rt.metrics().snapshot();

    // Run error backend.
    let handle = rt
        .run_streaming("error", mock_work_order())
        .await
        .expect("start");
    let _: Vec<_> = handle.events.collect().await;
    let _ = handle.receipt.await;

    // Error runs return early before recording metrics, so total_runs
    // should NOT increment (the runtime only records on receipt success).
    let snap_after_error = rt.metrics().snapshot();
    assert_eq!(
        snap_after_error.total_runs, snap_before.total_runs,
        "error run that returns early should not affect total_runs"
    );

    // Now run a successful backend.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("mock should succeed");
    assert!(receipt.receipt_sha256.is_some());

    let snap_final = rt.metrics().snapshot();
    assert_eq!(
        snap_final.total_runs,
        snap_before.total_runs + 1,
        "only the successful run should be counted"
    );
    assert!(snap_final.successful_runs >= 1, "at least one success");
}

// ---------- 18. Panic backend then state check ----------

#[tokio::test]
async fn panic_backend_metrics_and_registry_intact() {
    let mut rt = Runtime::new();
    rt.register_backend("panic", PanicBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);

    let names_before = rt.backend_names();

    let handle = rt
        .run_streaming("panic", mock_work_order())
        .await
        .expect("start");
    let _: Vec<_> = handle.events.collect().await;
    let _ = handle.receipt.await;

    // Registry unchanged.
    assert_eq!(rt.backend_names(), names_before);

    // Mock still works.
    let receipt = run_to_completion(&rt, "mock", mock_work_order())
        .await
        .expect("mock should work after panic");
    assert!(receipt.receipt_sha256.is_some());
}
