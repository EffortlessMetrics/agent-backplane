// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-runtime crate's orchestration pipeline.
//!
//! Categories:
//! 1. Runtime initialization and configuration
//! 2. Pipeline stages: workspace prep → policy check → backend selection → execution
//! 3. Event stream multiplexing
//! 4. Receipt generation and hashing
//! 5. Error propagation through pipeline
//! 6. Backend registration and lookup
//! 7. Edge cases

use std::sync::Arc;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    MinSupport, Outcome, PolicyProfile, WorkOrder, WorkOrderBuilder,
};
use abp_integrations::MockBackend;
use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker, BudgetViolation};
use abp_runtime::bus::{EventBus, FilteredSubscription};
use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};
use abp_runtime::hooks::{HookRegistry, LifecycleHook, LoggingHook, MetricsHook, ValidationHook};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter, MultiplexError};
use abp_runtime::observe::{ObservabilitySummary, RuntimeObserver, SpanStatus, TraceCollector};
use abp_runtime::pipeline::{AuditStage, Pipeline, PipelineStage, PolicyStage, ValidationStage};
use abp_runtime::registry::BackendRegistry;
use abp_runtime::retry::{RetryPolicy, TimeoutConfig};
use abp_runtime::stages::{
    DeduplicationStage, LoggingStage, MetricsStage, PipelineBuilder, RateLimitStage,
};
use abp_runtime::store::ReceiptStore;
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ============================================================
// Helpers
// ============================================================

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Creates a work order suitable for `run_streaming` — uses a small temp dir
/// with PassThrough mode so workspace staging doesn't try to copy large trees.
fn streaming_work_order(task: &str) -> (WorkOrder, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir for work order");
    let wo = WorkOrderBuilder::new(task)
        .root(dir.path().to_string_lossy().to_string())
        .workspace_mode(abp_core::WorkspaceMode::PassThrough)
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

// ============================================================
// 1. Runtime initialization and configuration
// ============================================================

#[test]
fn runtime_new_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn runtime_default_impl_equals_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn runtime_register_backend_adds_to_list() {
    let mut rt = Runtime::new();
    rt.register_backend("test-be", MockBackend);
    assert_eq!(rt.backend_names(), vec!["test-be"]);
}

#[test]
fn runtime_register_backend_replaces_existing() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("mock", MockBackend);
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn runtime_backend_lookup_returns_some() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn runtime_backend_lookup_returns_none_for_missing() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn runtime_registry_ref_lists_backends() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
}

#[test]
fn runtime_registry_mut_allows_modification() {
    let mut rt = Runtime::with_default_backends();
    rt.registry_mut().register("extra", MockBackend);
    assert!(rt.backend_names().contains(&"extra".to_string()));
}

#[test]
fn runtime_metrics_accessible() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
}

#[test]
fn runtime_projection_defaults_to_none() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_stream_pipeline_defaults_to_none() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_emulation_defaults_to_none() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

// ============================================================
// 2. Pipeline stages
// ============================================================

#[tokio::test]
async fn validation_stage_passes_valid_order() {
    let mut wo = simple_work_order("test task");
    let result = ValidationStage.process(&mut wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn validation_stage_rejects_empty_task() {
    let mut wo = WorkOrderBuilder::new("").build();
    let result = ValidationStage.process(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn validation_stage_rejects_whitespace_task() {
    let mut wo = WorkOrderBuilder::new("   ").build();
    let result = ValidationStage.process(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn validation_stage_rejects_empty_root() {
    let mut wo = WorkOrderBuilder::new("task").root("").build();
    let result = ValidationStage.process(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn validation_stage_name() {
    assert_eq!(ValidationStage.name(), "validation");
}

#[tokio::test]
async fn policy_stage_passes_no_conflict() {
    let mut wo = simple_work_order("test");
    let result = PolicyStage.process(&mut wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn policy_stage_rejects_conflicting_allow_deny() {
    let mut wo = WorkOrderBuilder::new("test")
        .policy(PolicyProfile {
            allowed_tools: vec!["bash".into()],
            disallowed_tools: vec!["bash".into()],
            ..Default::default()
        })
        .build();
    let result = PolicyStage.process(&mut wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn policy_stage_name() {
    assert_eq!(PolicyStage.name(), "policy");
}

#[tokio::test]
async fn audit_stage_records_entries() {
    let audit = AuditStage::new();
    let mut wo = simple_work_order("hello");
    let id = wo.id;
    audit.process(&mut wo).await.unwrap();
    let entries = audit.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].work_order_id, id);
    assert_eq!(entries[0].task, "hello");
}

#[tokio::test]
async fn audit_stage_default_impl() {
    let audit = AuditStage::default();
    assert_eq!(audit.entries().await.len(), 0);
}

#[tokio::test]
async fn audit_stage_name() {
    assert_eq!(AuditStage::new().name(), "audit");
}

#[tokio::test]
async fn pipeline_empty_executes_ok() {
    let p = Pipeline::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    let mut wo = simple_work_order("test");
    assert!(p.execute(&mut wo).await.is_ok());
}

#[tokio::test]
async fn pipeline_builder_pattern() {
    let p = Pipeline::new()
        .stage(ValidationStage)
        .stage(AuditStage::new());
    assert_eq!(p.len(), 2);
}

#[tokio::test]
async fn pipeline_short_circuits_on_failure() {
    let audit = Arc::new(tokio::sync::Mutex::new(false));
    let audit_clone = Arc::clone(&audit);

    struct FailStage;
    #[async_trait::async_trait]
    impl PipelineStage for FailStage {
        async fn process(&self, _: &mut WorkOrder) -> anyhow::Result<()> {
            anyhow::bail!("intentional failure");
        }
        fn name(&self) -> &str {
            "fail"
        }
    }

    struct MarkerStage(Arc<tokio::sync::Mutex<bool>>);
    #[async_trait::async_trait]
    impl PipelineStage for MarkerStage {
        async fn process(&self, _: &mut WorkOrder) -> anyhow::Result<()> {
            *self.0.lock().await = true;
            Ok(())
        }
        fn name(&self) -> &str {
            "marker"
        }
    }

    let p = Pipeline::new()
        .stage(FailStage)
        .stage(MarkerStage(audit_clone));
    let mut wo = simple_work_order("test");
    assert!(p.execute(&mut wo).await.is_err());
    assert!(!*audit.lock().await);
}

#[tokio::test]
async fn pipeline_default_is_empty() {
    let p = Pipeline::default();
    assert!(p.is_empty());
}

// ============================================================
// Stages module (extended)
// ============================================================

#[tokio::test]
async fn rate_limit_stage_allows_within_limit() {
    let stage = RateLimitStage::new(10);
    let mut wo = simple_work_order("test");
    assert!(stage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn rate_limit_stage_blocks_over_limit() {
    let stage = RateLimitStage::new(2);
    let mut wo = simple_work_order("test");
    stage.process(&mut wo).await.unwrap();
    stage.process(&mut wo).await.unwrap();
    let result = stage.process(&mut wo).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("rate limit"));
}

#[tokio::test]
async fn rate_limit_stage_name() {
    assert_eq!(RateLimitStage::new(5).name(), "rate_limit");
}

#[tokio::test]
async fn deduplication_stage_allows_first() {
    let stage = DeduplicationStage::new(Duration::from_secs(60));
    let mut wo = simple_work_order("test");
    assert!(stage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn deduplication_stage_blocks_duplicate() {
    let stage = DeduplicationStage::new(Duration::from_secs(60));
    let mut wo1 = simple_work_order("test");
    let mut wo2 = simple_work_order("test");
    stage.process(&mut wo1).await.unwrap();
    let result = stage.process(&mut wo2).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("duplicate"));
}

#[tokio::test]
async fn deduplication_stage_name() {
    assert_eq!(
        DeduplicationStage::new(Duration::from_secs(1)).name(),
        "deduplication"
    );
}

#[tokio::test]
async fn logging_stage_passes_through() {
    let stage = LoggingStage::new("TEST");
    let mut wo = simple_work_order("hello");
    assert!(stage.process(&mut wo).await.is_ok());
}

#[tokio::test]
async fn logging_stage_name() {
    assert_eq!(LoggingStage::new("x").name(), "logging");
}

#[tokio::test]
async fn metrics_stage_records_stats() {
    let stage = MetricsStage::new();
    let mut wo = simple_work_order("test");
    stage.process(&mut wo).await.unwrap();
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 1);
    assert_eq!(stats.successes, 1);
    assert_eq!(stats.failures, 0);
}

#[tokio::test]
async fn metrics_stage_default() {
    let stage = MetricsStage::default();
    let stats = stage.stats().await;
    assert_eq!(stats.invocations, 0);
}

#[tokio::test]
async fn metrics_stage_name() {
    assert_eq!(MetricsStage::new().name(), "metrics");
}

#[tokio::test]
async fn pipeline_builder_builds_stage_pipeline() {
    let sp = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(AuditStage::new()))
        .build();
    assert_eq!(sp.stage_names().len(), 2);
    assert_eq!(sp.stage_names()[0], "validation");
    assert_eq!(sp.stage_names()[1], "audit");
}

#[tokio::test]
async fn pipeline_builder_default() {
    let b = PipelineBuilder::default();
    assert_eq!(b.stage_count(), 0);
}

#[tokio::test]
async fn stage_pipeline_runs_all_stages() {
    let sp = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .add_stage(Box::new(MetricsStage::new()))
        .build();
    let mut wo = simple_work_order("test");
    let results = sp.execute(&mut wo).await;
    assert_eq!(results.len(), 2);
    assert!(results[0].passed);
    assert!(results[1].passed);
}

#[tokio::test]
async fn stage_pipeline_records_failure() {
    let sp = PipelineBuilder::new()
        .add_stage(Box::new(ValidationStage))
        .build();
    let mut wo = WorkOrderBuilder::new("").build();
    let results = sp.execute(&mut wo).await;
    assert_eq!(results.len(), 1);
    assert!(!results[0].passed);
    assert!(results[0].message.is_some());
}

// ============================================================
// 3. Event stream multiplexing
// ============================================================

#[tokio::test]
async fn multiplexer_broadcast_to_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    let count = mux.broadcast(ev.clone()).unwrap();
    assert_eq!(count, 1);
    let received = sub.recv().await.unwrap();
    assert!(matches!(received.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn multiplexer_no_subscribers_error() {
    let mux = EventMultiplexer::new(16);
    let ev = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    let result = mux.broadcast(ev);
    assert!(matches!(result, Err(MultiplexError::NoSubscribers)));
}

#[test]
fn multiplexer_subscriber_count() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
    let _sub1 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
    let _sub2 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 2);
}

#[tokio::test]
async fn multiplexer_multiple_subscribers_get_same_event() {
    let mux = EventMultiplexer::new(16);
    let mut sub1 = mux.subscribe();
    let mut sub2 = mux.subscribe();
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    mux.broadcast(ev).unwrap();
    assert!(sub1.recv().await.is_ok());
    assert!(sub2.recv().await.is_ok());
}

#[test]
fn multiplexer_try_recv_returns_none_when_empty() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    assert!(sub.try_recv().is_none());
}

#[test]
fn event_router_empty_routes() {
    let router = EventRouter::new();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn event_router_default_impl() {
    let router = EventRouter::default();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn event_router_routes_by_kind() {
    let mut router = EventRouter::new();
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);
    router.add_route(
        "assistant_message",
        Box::new(move |_| {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    let ev = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    router.route(&ev);
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn event_router_ignores_unregistered_kind() {
    let mut router = EventRouter::new();
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);
    router.add_route(
        "run_started",
        Box::new(move |_| {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "ignored".into(),
    });
    router.route(&ev);
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 0);
}

// ============================================================
// Event bus
// ============================================================

#[test]
fn event_bus_new_has_no_subscribers() {
    let bus = EventBus::new();
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn event_bus_with_capacity() {
    let bus = EventBus::with_capacity(64);
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn event_bus_default_impl() {
    let bus = EventBus::default();
    assert_eq!(bus.stats().total_published, 0);
}

#[test]
fn event_bus_publish_without_subscribers_counts_dropped() {
    let bus = EventBus::new();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    bus.publish(ev);
    let stats = bus.stats();
    assert_eq!(stats.total_published, 1);
    assert_eq!(stats.dropped_events, 1);
}

#[tokio::test]
async fn event_bus_subscribe_and_receive() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    bus.publish(ev);
    let received = sub.recv().await;
    assert!(received.is_some());
}

#[tokio::test]
async fn event_bus_filtered_subscription() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev: &AgentEvent| matches!(ev.kind, AgentEventKind::AssistantMessage { .. })),
    );

    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "skip".into(),
    }));
    bus.publish(make_event(AgentEventKind::AssistantMessage {
        text: "keep".into(),
    }));

    // Drop bus to close channel after sending.
    drop(bus);

    let received = filtered.recv().await;
    assert!(received.is_some());
    assert!(matches!(
        received.unwrap().kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn event_bus_stats_snapshot() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    let stats = bus.stats();
    assert_eq!(stats.total_published, 0);
    assert_eq!(stats.active_subscribers, 1);
    assert_eq!(stats.dropped_events, 0);
}

// ============================================================
// 4. Receipt generation and hashing
// ============================================================

#[tokio::test]
async fn run_streaming_produces_receipt_with_hash() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("hash test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    while events.next().await.is_some() {}
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

#[tokio::test]
async fn receipt_hash_is_deterministic_for_same_receipt() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let hash1 = abp_core::receipt_hash(&receipt).unwrap();
    let hash2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2);
}

#[tokio::test]
async fn run_streaming_receipt_has_trace_events() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("trace test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    while events.next().await.is_some() {}
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn run_streaming_receipt_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("outcome test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    while events.next().await.is_some() {}
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ============================================================
// Receipt store
// ============================================================

#[test]
fn receipt_store_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();
    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
}

#[test]
fn receipt_store_list_after_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], r.meta.run_id);
}

#[test]
fn receipt_store_verify_valid_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    store.save(&r).unwrap();
    assert!(store.verify(r.meta.run_id).unwrap());
}

#[test]
fn receipt_store_verify_chain_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let verification = store.verify_chain().unwrap();
    assert!(verification.is_valid);
    assert_eq!(verification.valid_count, 0);
}

#[test]
fn receipt_store_verify_chain_with_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for _ in 0..3 {
        let r = abp_core::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        store.save(&r).unwrap();
    }
    let verification = store.verify_chain().unwrap();
    assert!(verification.is_valid);
    assert_eq!(verification.valid_count, 3);
    assert!(verification.invalid_hashes.is_empty());
}

#[test]
fn receipt_store_load_nonexistent_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let result = store.load(uuid::Uuid::new_v4());
    assert!(result.is_err());
}

// ============================================================
// 5. Error propagation through pipeline
// ============================================================

#[tokio::test]
async fn run_streaming_unknown_backend_error() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("test");
    let result = rt.run_streaming("nonexistent", wo).await;
    match result {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent");
        }
        Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        Ok(_) => panic!("expected UnknownBackend error, got Ok"),
    }
}

#[test]
fn runtime_error_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert!(err.to_string().contains("foo"));
}

#[test]
fn runtime_error_workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.to_string().contains("workspace"));
}

#[test]
fn runtime_error_policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(err.to_string().contains("policy"));
}

#[test]
fn runtime_error_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(err.to_string().contains("backend"));
}

#[test]
fn runtime_error_capability_check_failed_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing tool".into());
    assert!(err.to_string().contains("capability"));
}

#[test]
fn runtime_error_has_error_codes() {
    assert_eq!(
        RuntimeError::UnknownBackend { name: "x".into() }.error_code(),
        abp_error::ErrorCode::BackendNotFound
    );
    assert_eq!(
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("")).error_code(),
        abp_error::ErrorCode::WorkspaceInitFailed
    );
    assert_eq!(
        RuntimeError::PolicyFailed(anyhow::anyhow!("")).error_code(),
        abp_error::ErrorCode::PolicyInvalid
    );
    assert_eq!(
        RuntimeError::BackendFailed(anyhow::anyhow!("")).error_code(),
        abp_error::ErrorCode::BackendCrashed
    );
    assert_eq!(
        RuntimeError::CapabilityCheckFailed("".into()).error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn runtime_error_into_abp_error_preserves_code() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let code = err.error_code();
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, code);
}

#[test]
fn classified_error_roundtrip() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
}

// ============================================================
// 6. Backend registration and lookup
// ============================================================

#[test]
fn registry_new_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.get("mock").is_some());
}

#[test]
fn registry_get_arc() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.get_arc("mock").is_some());
}

#[test]
fn registry_contains() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.contains("mock"));
    assert!(!reg.contains("other"));
}

#[test]
fn registry_list_sorted() {
    let mut reg = BackendRegistry::default();
    reg.register("z-backend", MockBackend);
    reg.register("a-backend", MockBackend);
    let names = reg.list();
    assert_eq!(names, vec!["a-backend", "z-backend"]);
}

#[test]
fn registry_remove() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    let removed = reg.remove("mock");
    assert!(removed.is_some());
    assert!(!reg.contains("mock"));
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = BackendRegistry::default();
    assert!(reg.remove("nothing").is_none());
}

#[test]
fn registry_get_returns_none_for_missing() {
    let reg = BackendRegistry::default();
    assert!(reg.get("missing").is_none());
}

#[test]
fn registry_register_multiple() {
    let mut reg = BackendRegistry::default();
    reg.register("a", MockBackend);
    reg.register("b", MockBackend);
    reg.register("c", MockBackend);
    assert_eq!(reg.list().len(), 3);
}

// ============================================================
// Capability checks
// ============================================================

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
fn check_capabilities_empty_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn check_capabilities_unknown_backend() {
    let rt = Runtime::new();
    let reqs = CapabilityRequirements::default();
    match rt.check_capabilities("missing", &reqs) {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "missing");
        }
        other => panic!("expected UnknownBackend, got {other:?}"),
    }
}

// ============================================================
// 7. Edge cases
// ============================================================

#[tokio::test]
async fn run_streaming_provides_run_id() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert!(!handle.run_id.is_nil());
    let mut events = handle.events;
    while events.next().await.is_some() {}
    let _ = handle.receipt.await;
}

#[tokio::test]
async fn run_streaming_events_stream_has_events() {
    let rt = Runtime::with_default_backends();
    let (wo, _dir) = streaming_work_order("stream test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while events.next().await.is_some() {
        count += 1;
    }
    assert!(count > 0);
    let _ = handle.receipt.await;
}

#[tokio::test]
async fn multiple_sequential_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let (wo, _dir) = streaming_work_order(&format!("run {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let mut events = handle.events;
        while events.next().await.is_some() {}
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    for _ in 0..3 {
        let (wo, _dir) = streaming_work_order("chain test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let mut events = handle.events;
        while events.next().await.is_some() {}
        let _ = handle.receipt.await.unwrap().unwrap();
    }
    let chain = rt.receipt_chain();
    let locked = chain.lock().await;
    assert_eq!(locked.len(), 3);
}

// ============================================================
// Hooks
// ============================================================

#[test]
fn hook_registry_empty() {
    let reg = HookRegistry::new();
    assert_eq!(reg.hook_count(), 0);
    assert!(reg.hook_names().is_empty());
}

#[test]
fn hook_registry_default() {
    let reg = HookRegistry::default();
    assert_eq!(reg.hook_count(), 0);
}

#[test]
fn hook_registry_register_and_fire() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    assert_eq!(reg.hook_count(), 1);
    assert_eq!(reg.hook_names(), vec!["logging"]);
}

#[test]
fn hook_registry_fire_run_start() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    let wo = simple_work_order("test");
    let results = reg.fire_run_start(&wo);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn hook_registry_fire_run_start_invalid() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(ValidationHook));
    let wo = WorkOrderBuilder::new("").build();
    let results = reg.fire_run_start(&wo);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn hook_registry_fire_event() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let ev = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    let results = reg.fire_event(&ev);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn hook_registry_fire_run_complete() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let results = reg.fire_run_complete(&receipt);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn hook_registry_fire_error() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(LoggingHook));
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    reg.fire_error(&err);
}

#[test]
fn metrics_hook_records_run() {
    let metrics = Arc::new(RunMetrics::new());
    let hook = MetricsHook::new(Arc::clone(&metrics));
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    hook.on_run_complete(&receipt).unwrap();
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
}

#[test]
fn validation_hook_rejects_empty_task() {
    let hook = ValidationHook;
    let wo = WorkOrderBuilder::new("").build();
    assert!(hook.on_run_start(&wo).is_err());
}

#[test]
fn validation_hook_passes_valid() {
    let hook = ValidationHook;
    let wo = simple_work_order("valid task");
    assert!(hook.on_run_start(&wo).is_ok());
}

// ============================================================
// Cancellation
// ============================================================

#[test]
fn cancellation_token_starts_uncancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel() {
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
fn cancellation_token_default() {
    let token = CancellationToken::default();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellable_run_records_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::UserRequested);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(CancellationReason::UserRequested));
}

#[test]
fn cancellable_run_keeps_first_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::Timeout);
    run.cancel(CancellationReason::BudgetExhausted);
    assert_eq!(run.reason(), Some(CancellationReason::Timeout));
}

#[test]
fn cancellation_reason_descriptions() {
    let reasons = vec![
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

#[test]
fn cancellation_reason_serde_roundtrip() {
    let reason = CancellationReason::Timeout;
    let json = serde_json::to_string(&reason).unwrap();
    let back: CancellationReason = serde_json::from_str(&json).unwrap();
    assert_eq!(back, reason);
}

// ============================================================
// Budget
// ============================================================

#[test]
fn budget_tracker_within_limits_no_caps() {
    let t = BudgetTracker::new(BudgetLimit::default());
    t.record_tokens(999);
    assert_eq!(t.check(), BudgetStatus::WithinLimits);
}

#[test]
fn budget_tracker_tokens_exceeded() {
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
fn budget_tracker_cost_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_cost_usd: Some(1.0),
        ..Default::default()
    });
    t.record_cost(1.1);
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::CostExceeded { .. })
    ));
}

#[test]
fn budget_tracker_turns_exceeded() {
    let t = BudgetTracker::new(BudgetLimit {
        max_turns: Some(2),
        ..Default::default()
    });
    t.record_turn();
    t.record_turn();
    t.record_turn();
    assert!(matches!(
        t.check(),
        BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded { .. })
    ));
}

#[test]
fn budget_tracker_remaining() {
    let t = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        max_turns: Some(5),
        ..Default::default()
    });
    t.record_tokens(30);
    t.record_turn();
    let rem = t.remaining();
    assert_eq!(rem.tokens, Some(70));
    assert_eq!(rem.turns, Some(4));
}

#[test]
fn budget_violation_display() {
    let v = BudgetViolation::TokensExceeded {
        used: 200,
        limit: 100,
    };
    let s = v.to_string();
    assert!(s.contains("200"));
    assert!(s.contains("100"));
}

// ============================================================
// Observability
// ============================================================

#[test]
fn trace_collector_empty() {
    let tc = TraceCollector::new();
    assert!(tc.spans().is_empty());
    assert!(tc.active_spans().is_empty());
    assert!(tc.root_spans().is_empty());
}

#[test]
fn trace_collector_start_and_end_span() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("test-span");
    assert_eq!(tc.spans().len(), 1);
    assert_eq!(tc.active_spans().len(), 1);
    tc.end_span(&id);
    assert_eq!(tc.active_spans().len(), 0);
}

#[test]
fn trace_collector_child_spans() {
    let mut tc = TraceCollector::new();
    let parent = tc.start_span("parent");
    let _child = tc.start_child_span("child", &parent);
    assert_eq!(tc.root_spans().len(), 1);
    assert_eq!(tc.children(&parent).len(), 1);
}

#[test]
fn trace_collector_set_status() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("test");
    tc.set_status(
        &id,
        SpanStatus::Error {
            message: "fail".into(),
        },
    );
    assert!(matches!(tc.spans()[0].status, SpanStatus::Error { .. }));
}

#[test]
fn trace_collector_set_attribute() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("test");
    tc.set_attribute(&id, "key", "value");
    assert_eq!(tc.spans()[0].attributes.get("key").unwrap(), "value");
}

#[test]
fn trace_collector_to_json() {
    let tc = TraceCollector::new();
    let json = tc.to_json();
    assert_eq!(json, "[]");
}

#[test]
fn runtime_observer_empty() {
    let obs = RuntimeObserver::new();
    assert!(obs.metrics().is_empty());
    let summary = obs.summary();
    assert_eq!(
        summary,
        ObservabilitySummary {
            total_spans: 0,
            active_spans: 0,
            error_spans: 0,
            metrics_count: 0,
        }
    );
}

#[test]
fn runtime_observer_record_metric() {
    let mut obs = RuntimeObserver::new();
    obs.record_metric("latency_ms", 42.0);
    assert_eq!(obs.metrics().get("latency_ms"), Some(&42.0));
    assert_eq!(obs.summary().metrics_count, 1);
}

#[test]
fn runtime_observer_trace_collector_access() {
    let mut obs = RuntimeObserver::new();
    let id = obs.trace_collector().start_span("test");
    obs.trace_collector().end_span(&id);
    assert_eq!(obs.summary().total_spans, 1);
    assert_eq!(obs.summary().active_spans, 0);
}

// ============================================================
// Telemetry
// ============================================================

#[test]
fn run_metrics_snapshot_initial() {
    let m = RunMetrics::new();
    let s = m.snapshot();
    assert_eq!(s.total_runs, 0);
    assert_eq!(s.successful_runs, 0);
    assert_eq!(s.failed_runs, 0);
    assert_eq!(s.total_events, 0);
}

#[test]
fn run_metrics_record_run() {
    let m = RunMetrics::new();
    m.record_run(100, true, 10);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1);
    assert_eq!(s.successful_runs, 1);
    assert_eq!(s.total_events, 10);
}

#[test]
fn run_metrics_record_failed_run() {
    let m = RunMetrics::new();
    m.record_run(50, false, 2);
    let s = m.snapshot();
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

// ============================================================
// Retry
// ============================================================

#[test]
fn retry_policy_default() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
}

#[test]
fn retry_policy_builder() {
    let p = RetryPolicy::builder()
        .max_retries(5)
        .initial_backoff(Duration::from_millis(200))
        .max_backoff(Duration::from_secs(10))
        .backoff_multiplier(3.0)
        .build();
    assert_eq!(p.max_retries, 5);
}

#[test]
fn retry_policy_should_retry() {
    let p = RetryPolicy::default();
    assert!(p.should_retry(0));
    assert!(p.should_retry(2));
    assert!(!p.should_retry(3));
}

#[test]
fn retry_policy_compute_delay_bounded() {
    let p = RetryPolicy::default();
    for attempt in 0..10 {
        let delay = p.compute_delay(attempt);
        assert!(delay <= p.max_backoff);
    }
}

#[test]
fn timeout_config_defaults() {
    let tc = TimeoutConfig::default();
    assert!(tc.run_timeout.is_none());
    assert!(tc.event_timeout.is_none());
}

#[test]
fn retry_policy_serde_roundtrip() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let back: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

// ============================================================
// Stream pipeline integration
// ============================================================

#[test]
fn apply_pipeline_none_passes_through() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "test".into(),
    });
    let result = abp_runtime::stream::apply_pipeline(None, ev);
    assert!(result.is_some());
}

// ============================================================
// NoProjectionMatch error
// ============================================================

#[test]
fn select_backend_without_projection() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("test");
    match rt.select_backend(&wo) {
        Err(RuntimeError::NoProjectionMatch { .. }) => {}
        other => panic!("expected NoProjectionMatch, got {other:?}"),
    }
}
