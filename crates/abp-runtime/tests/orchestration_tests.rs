// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep runtime orchestration tests covering the full pipeline.
//!
//! These tests exercise: Runtime → Backend → Receipt, Store integration,
//! Registry, Telemetry, Pipeline stages, Hooks, Multiplexer, Retry policy,
//! error propagation, capability pre-checks, and receipt chain verification.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ContextPacket, ExecutionLane, MinSupport, Outcome, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::hooks::{HookRegistry, LifecycleHook};
use abp_runtime::multiplex::EventMultiplexer;
use abp_runtime::pipeline::{Pipeline, PolicyStage, ValidationStage};
use abp_runtime::retry::RetryPolicy;
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> WorkOrder {
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

/// Run a work order to completion, collecting events and the receipt.
async fn run_to_completion(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming(backend, wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

/// Counting hook that tracks on_start / on_event / on_complete calls.
struct CountingHook {
    starts: AtomicU32,
    events: AtomicU32,
    completes: AtomicU32,
}

impl CountingHook {
    fn new() -> Self {
        Self {
            starts: AtomicU32::new(0),
            events: AtomicU32::new(0),
            completes: AtomicU32::new(0),
        }
    }
}

impl LifecycleHook for CountingHook {
    fn on_run_start(
        &self,
        _wo: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn on_event(
        &self,
        _event: &AgentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.events.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn on_run_complete(
        &self,
        _receipt: &abp_core::Receipt,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.completes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn name(&self) -> &str {
        "counting"
    }
}

// ===========================================================================
// 1. Full pipeline: WorkOrder → Runtime → Receipt with valid hash
// ===========================================================================

#[tokio::test]
async fn full_pipeline_produces_valid_hashed_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("full pipeline test");
    let wo_id = wo.id;

    let (events, receipt) = run_to_completion(&rt, "mock", wo).await;

    // Receipt metadata
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
    assert!(matches!(receipt.outcome, Outcome::Complete));

    // Hash is present and self-consistent
    let stored_hash = receipt.receipt_sha256.clone().expect("hash must exist");
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(stored_hash, recomputed);

    // Events include RunStarted and RunCompleted
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

// ===========================================================================
// 2. Store integration: run → store receipt → retrieve by ID → verify
// ===========================================================================

#[tokio::test]
async fn store_integration_save_load_verify() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("store integration test");
    let (_events, receipt) = run_to_completion(&rt, "mock", wo).await;

    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    store.save(&receipt).unwrap();
    let loaded = store.load(receipt.meta.run_id).unwrap();

    assert_eq!(loaded.meta.run_id, receipt.meta.run_id);
    assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256);
    assert!(store.verify(receipt.meta.run_id).unwrap());
}

// ===========================================================================
// 3. Registry: register multiple backends → select by name → run on each
// ===========================================================================

#[tokio::test]
async fn registry_multiple_backends_run_each() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("beta", abp_integrations::MockBackend);
    rt.register_backend("gamma", abp_integrations::MockBackend);

    let names = rt.backend_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
    assert!(names.contains(&"gamma".to_string()));

    for name in &names {
        let (_, receipt) = run_to_completion(&rt, name, make_work_order("registry test")).await;
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(receipt.receipt_sha256.is_some());
    }
}

// ===========================================================================
// 4. Telemetry: run → check metrics (event count, duration)
// ===========================================================================

#[tokio::test]
async fn telemetry_metrics_after_run() {
    let rt = Runtime::with_default_backends();

    let snap_before = rt.metrics().snapshot();
    assert_eq!(snap_before.total_runs, 0);

    let (_events, _receipt) =
        run_to_completion(&rt, "mock", make_work_order("telemetry test")).await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0, "should have recorded events");
}

// ===========================================================================
// 5. Pipeline stages: run with validation + policy stages → verify executed
// ===========================================================================

#[tokio::test]
async fn pipeline_stages_validation_and_policy() {
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);

    assert_eq!(pipeline.len(), 2);

    // Valid work order passes
    let mut wo = make_work_order("pipeline stage test");
    pipeline
        .execute(&mut wo)
        .await
        .expect("pipeline should pass");

    // Empty task fails validation
    let mut bad_wo = make_work_order("");
    let err = pipeline.execute(&mut bad_wo).await;
    assert!(err.is_err(), "empty task should fail validation");
}

// ===========================================================================
// 6. Hooks firing: register hooks → run → verify on_start/on_event/on_complete
// ===========================================================================

#[tokio::test]
async fn hooks_firing_on_start_event_complete() {
    let hook = Arc::new(CountingHook::new());

    // Fire hooks manually against a work order and receipt
    let mut reg = HookRegistry::new();
    // We need to use a wrapper because HookRegistry takes Box<dyn LifecycleHook>
    // but our CountingHook is behind Arc. Use a forwarding wrapper.
    let hook_clone = Arc::clone(&hook);
    struct ArcHook(Arc<CountingHook>);
    impl LifecycleHook for ArcHook {
        fn on_run_start(
            &self,
            wo: &WorkOrder,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.0.on_run_start(wo)
        }
        fn on_event(
            &self,
            event: &AgentEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.0.on_event(event)
        }
        fn on_run_complete(
            &self,
            receipt: &abp_core::Receipt,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.0.on_run_complete(receipt)
        }
        fn name(&self) -> &str {
            "arc-counting"
        }
    }
    reg.register(Box::new(ArcHook(hook_clone)));

    let wo = make_work_order("hooks test");
    let results = reg.fire_run_start(&wo);
    assert!(results.iter().all(Result::is_ok));

    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    reg.fire_event(&event);
    reg.fire_event(&event);

    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    reg.fire_run_complete(&receipt);

    assert_eq!(hook.starts.load(Ordering::Relaxed), 1);
    assert_eq!(hook.events.load(Ordering::Relaxed), 2);
    assert_eq!(hook.completes.load(Ordering::Relaxed), 1);
}

// ===========================================================================
// 7. Multiplexer: subscribe → broadcast → verify subscriber receives events
// ===========================================================================

#[tokio::test]
async fn multiplexer_subscriber_receives_events() {
    let mux = EventMultiplexer::new(32);
    let mut sub = mux.subscribe();

    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "mux test".into(),
        },
        ext: None,
    };
    mux.broadcast(ev).unwrap();

    let received = sub.recv().await.unwrap();
    assert!(matches!(received.kind, AgentEventKind::RunStarted { .. }));
}

// ===========================================================================
// 8. Retry policy: verify retry behavior with compute_delay and should_retry
// ===========================================================================

#[tokio::test]
async fn retry_policy_behavior() {
    let policy = RetryPolicy::builder()
        .max_retries(3)
        .initial_backoff(std::time::Duration::from_millis(100))
        .max_backoff(std::time::Duration::from_secs(2))
        .backoff_multiplier(2.0)
        .build();

    // should_retry for attempts 0..2 is true, attempt 3 is false
    assert!(policy.should_retry(0));
    assert!(policy.should_retry(1));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3));

    // Delays increase with each attempt
    let d0 = policy.compute_delay(0);
    let d1 = policy.compute_delay(1);
    let d2 = policy.compute_delay(2);
    assert!(d1 > d0, "delay should increase: d0={d0:?} d1={d1:?}");
    assert!(d2 > d1, "delay should increase: d1={d1:?} d2={d2:?}");

    // All delays are bounded by max_backoff
    for attempt in 0..10 {
        let d = policy.compute_delay(attempt);
        assert!(d <= std::time::Duration::from_secs(2));
    }
}

// ===========================================================================
// 9. Error propagation: unknown backend → RuntimeError::UnknownBackend
// ===========================================================================

#[tokio::test]
async fn error_propagation_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("error test");

    let result = rt.run_streaming("nonexistent_backend", wo).await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected error for unknown backend"),
    };
    assert!(
        matches!(&err, RuntimeError::UnknownBackend { name } if name == "nonexistent_backend"),
        "expected UnknownBackend, got {err:?}"
    );
}

// ===========================================================================
// 10. Capability pre-check: backend missing required caps → error
// ===========================================================================

#[tokio::test]
async fn capability_precheck_fails_for_missing_caps() {
    let rt = Runtime::with_default_backends();
    let mut wo = make_work_order("capability check test");
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let err = match rt.run_streaming("mock", wo).await {
        Err(e) => e,
        Ok(_) => panic!("expected error for unsatisfiable capability"),
    };
    let err_msg = format!("{err}");
    assert!(
        err_msg.contains("capability") || err_msg.contains("Capability"),
        "error should mention capability: {err_msg}"
    );
}

// ===========================================================================
// 11. Sequential runs: 3 runs in sequence → all succeed with unique IDs
// ===========================================================================

#[tokio::test]
async fn sequential_runs_unique_ids() {
    let rt = Runtime::with_default_backends();
    let mut run_ids = HashSet::new();
    let mut receipts = Vec::new();

    for i in 0..3 {
        let wo = make_work_order(&format!("sequential run {i}"));
        let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
        run_ids.insert(handle.run_id);
        let _: Vec<_> = handle.events.collect().await;
        let receipt = handle.receipt.await.expect("join").expect("receipt");
        assert!(matches!(receipt.outcome, Outcome::Complete));
        receipts.push(receipt);
    }

    assert_eq!(run_ids.len(), 3, "each run must have a unique run_id");
    for r in &receipts {
        assert!(r.receipt_sha256.is_some());
    }
}

// ===========================================================================
// 12. Concurrent subscriptions: multiple subscribers receive same events
// ===========================================================================

#[tokio::test]
async fn concurrent_subscriptions_receive_same_events() {
    let mux = Arc::new(EventMultiplexer::new(64));
    let mut handles = Vec::new();

    for _ in 0..5 {
        let mut sub = mux.subscribe();
        handles.push(tokio::spawn(async move {
            let ev = sub.recv().await.unwrap();
            matches!(ev.kind, AgentEventKind::RunStarted { .. })
        }));
    }

    tokio::task::yield_now().await;
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "broadcast".into(),
        },
        ext: None,
    };
    let receivers = mux.broadcast(ev).unwrap();
    assert_eq!(receivers, 5);

    for h in handles {
        assert!(
            h.await.unwrap(),
            "subscriber should have received RunStarted"
        );
    }
}

// ===========================================================================
// 13. Receipt chain: multiple runs → build chain → verify hashes
// ===========================================================================

#[tokio::test]
async fn receipt_chain_multiple_runs() {
    let rt = Runtime::with_default_backends();
    let mut receipts = Vec::new();

    for i in 0..3 {
        let wo = make_work_order(&format!("chain run {i}"));
        let (_, receipt) = run_to_completion(&rt, "mock", wo).await;
        receipts.push(receipt);
    }

    // All receipts have valid self-consistent hashes
    for r in &receipts {
        let hash = r.receipt_sha256.as_ref().expect("hash");
        let recomputed = abp_core::receipt_hash(r).expect("recompute");
        assert_eq!(hash, &recomputed);
    }

    // All run_ids are unique
    let ids: HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 3);
}

// ===========================================================================
// 14. Store chain verification: store 3 receipts → chain.verify() succeeds
// ===========================================================================

#[tokio::test]
async fn store_chain_verification() {
    let rt = Runtime::with_default_backends();
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    for i in 0..3 {
        let wo = make_work_order(&format!("chain verify {i}"));
        let (_, receipt) = run_to_completion(&rt, "mock", wo).await;
        store.save(&receipt).unwrap();
    }

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 3);

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid, "chain should be valid");
    assert_eq!(chain.valid_count, 3);
    assert!(chain.invalid_hashes.is_empty());
    assert_eq!(chain.gaps.len(), 2, "3 receipts should produce 2 gaps");
}

// ===========================================================================
// 15. Run metrics: after run, metrics reflect correct counts
// ===========================================================================

#[tokio::test]
async fn run_metrics_correct_counts() {
    let rt = Runtime::with_default_backends();

    // Run twice
    for i in 0..2 {
        let wo = make_work_order(&format!("metrics run {i}"));
        let (_, _receipt) = run_to_completion(&rt, "mock", wo).await;
    }

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events >= 2, "should have at least some events");
    // average_run_duration_ms may be 0 for fast mock runs, but should not panic
    let _ = snap.average_run_duration_ms;
}

// ===========================================================================
// 16. Pipeline rejects conflicting policy (tool in both allow and deny)
// ===========================================================================

#[tokio::test]
async fn pipeline_policy_rejects_conflicting_tools() {
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);

    let mut wo = make_work_order("policy conflict test");
    wo.policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };

    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err(), "conflicting tool policy should fail");
}

// ===========================================================================
// 17. Registry contains and remove
// ===========================================================================

#[tokio::test]
async fn registry_contains_and_remove() {
    let mut rt = Runtime::new();
    rt.register_backend("temp", abp_integrations::MockBackend);

    assert!(rt.registry().contains("temp"));
    assert!(!rt.registry().contains("nonexistent"));

    let removed = rt.registry_mut().remove("temp");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("temp"));
}

// ===========================================================================
// 18. Metrics snapshot is zero-initialized
// ===========================================================================

#[tokio::test]
async fn metrics_zero_initialized() {
    let metrics = RunMetrics::new();
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
    assert_eq!(snap.average_run_duration_ms, 0);
}

// ===========================================================================
// 19. Multiplexer broadcast with multiple sequential events
// ===========================================================================

#[tokio::test]
async fn multiplexer_sequential_events() {
    let mux = EventMultiplexer::new(32);
    let mut sub = mux.subscribe();

    let kinds = [
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ];

    for kind in &kinds {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: kind.clone(),
            ext: None,
        };
        mux.broadcast(ev).unwrap();
    }

    let e1 = sub.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    let e2 = sub.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::AssistantMessage { .. }));
    let e3 = sub.recv().await.unwrap();
    assert!(matches!(e3.kind, AgentEventKind::RunCompleted { .. }));
}

// ===========================================================================
// 20. Full pipeline receipt trace is non-empty
// ===========================================================================

#[tokio::test]
async fn receipt_trace_is_populated() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("trace test");

    let (events, receipt) = run_to_completion(&rt, "mock", wo).await;

    assert!(!events.is_empty(), "event stream should not be empty");
    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should not be empty"
    );
}
