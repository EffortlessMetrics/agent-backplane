// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the abp-runtime crate covering orchestration logic.

use abp_core::{
    AgentEvent, AgentEventKind, CapabilityRequirements, ExecutionLane, Outcome, PolicyProfile,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::bus::{EventBus, FilteredSubscription};
use abp_runtime::cancel::{CancellableRun, CancellationToken};
use abp_runtime::hooks::{HookRegistry, LifecycleHook, LoggingHook, ValidationHook};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter, MultiplexError};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use chrono::Utc;
use std::sync::Arc;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "deep test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

async fn run_to_completion(rt: &Runtime, wo: WorkOrder) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ===========================================================================
// 1. Runtime construction
// ===========================================================================

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
fn runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[test]
fn runtime_with_default_backends_mock_identity() {
    let rt = Runtime::with_default_backends();
    let backend = rt.backend("mock").expect("mock backend exists");
    assert_eq!(backend.identity().id, "mock");
}

// ===========================================================================
// 2. RuntimeError variants — construct and Display
// ===========================================================================

#[test]
fn error_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend {
        name: "foobar".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("foobar"), "display should contain name: {msg}");
    assert!(
        msg.contains("unknown backend"),
        "display should mention unknown backend: {msg}"
    );
}

#[test]
fn error_workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    let msg = err.to_string();
    assert!(
        msg.contains("workspace preparation failed"),
        "display: {msg}"
    );
}

#[test]
fn error_policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    let msg = err.to_string();
    assert!(msg.contains("policy compilation failed"), "display: {msg}");
}

#[test]
fn error_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    let msg = err.to_string();
    assert!(msg.contains("backend execution failed"), "display: {msg}");
}

#[test]
fn error_capability_check_failed_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    let msg = err.to_string();
    assert!(msg.contains("missing mcp"), "display: {msg}");
}

#[test]
fn error_no_projection_match_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("no matrix"), "display: {msg}");
    assert!(msg.contains("projection failed"), "display: {msg}");
}

// ===========================================================================
// 3. RuntimeError implements std::error::Error / source chain
// ===========================================================================

#[test]
fn error_trait_is_implemented() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    // Must be usable as &dyn Error
    let _dyn_err: &dyn std::error::Error = &err;
}

#[test]
fn workspace_failed_has_source() {
    let inner = anyhow::anyhow!("disk full");
    let err = RuntimeError::WorkspaceFailed(inner);
    let source = std::error::Error::source(&err);
    assert!(source.is_some(), "WorkspaceFailed should have a source");
}

#[test]
fn policy_failed_has_source() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("glob error"));
    assert!(
        std::error::Error::source(&err).is_some(),
        "PolicyFailed should have a source"
    );
}

#[test]
fn backend_failed_has_source() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
    assert!(
        std::error::Error::source(&err).is_some(),
        "BackendFailed should have a source"
    );
}

#[test]
fn unknown_backend_has_no_source() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(
        std::error::Error::source(&err).is_none(),
        "UnknownBackend should not have a source"
    );
}

// ===========================================================================
// 4. Backend registration and lookup
// ===========================================================================

#[test]
fn register_and_lookup_mock() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    assert!(rt.backend("mock").is_some());
}

#[test]
fn lookup_missing_backend_returns_none() {
    let rt = Runtime::new();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_names_are_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zeta", abp_integrations::MockBackend);
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("mid", abp_integrations::MockBackend);
    let names = rt.backend_names();
    assert_eq!(names, vec!["alpha", "mid", "zeta"]);
}

#[test]
fn register_replaces_previous() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);
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
    assert!(rt.registry().contains("mock"));
    let removed = rt.registry_mut().remove("mock");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("mock"));
}

// ===========================================================================
// 5. WorkOrder preparation — end-to-end with mock
// ===========================================================================

#[tokio::test]
async fn run_streaming_produces_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let wo_id = wo.id;
    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn run_streaming_with_passthrough_workspace() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let (_, receipt) = run_to_completion(&rt, wo).await;
    assert!(receipt.receipt_sha256.is_some());
}

// ===========================================================================
// 6. Event multiplexing concepts
// ===========================================================================

#[tokio::test]
async fn event_bus_single_subscriber() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    });
    bus.publish(ev);
    let received = sub.recv().await;
    assert!(received.is_some());
    assert!(matches!(
        received.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn event_bus_multiple_subscribers() {
    let bus = EventBus::new();
    let mut sub1 = bus.subscribe();
    let mut sub2 = bus.subscribe();
    bus.publish(make_event(AgentEventKind::Warning {
        message: "test".into(),
    }));
    assert!(sub1.recv().await.is_some());
    assert!(sub2.recv().await.is_some());
}

#[test]
fn event_bus_stats_tracking() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    }));
    bus.publish(make_event(AgentEventKind::RunCompleted {
        message: "b".into(),
    }));
    let stats = bus.stats();
    assert_eq!(stats.total_published, 2);
    assert_eq!(stats.active_subscribers, 1);
}

#[test]
fn event_bus_no_subscribers_counts_dropped() {
    let bus = EventBus::new();
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    }));
    let stats = bus.stats();
    assert_eq!(stats.total_published, 1);
    assert_eq!(stats.dropped_events, 1);
}

#[tokio::test]
async fn multiplexer_broadcast_and_receive() {
    let mux = EventMultiplexer::new(64);
    let mut sub = mux.subscribe();
    let count = mux
        .broadcast(make_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        }))
        .unwrap();
    assert_eq!(count, 1);
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn multiplexer_no_subscribers_error() {
    let mux = EventMultiplexer::new(64);
    let result = mux.broadcast(make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    }));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), MultiplexError::NoSubscribers));
}

// ===========================================================================
// 7. Receipt generation — canonical receipt from run results
// ===========================================================================

#[tokio::test]
async fn receipt_has_valid_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_to_completion(&rt, mock_work_order()).await;
    let hash = receipt.receipt_sha256.as_ref().expect("hash must exist");
    assert!(!hash.is_empty());
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute");
    assert_eq!(hash, &recomputed);
}

#[tokio::test]
async fn receipt_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// 8. RunHandle contains receiver + join handle
// ===========================================================================

#[tokio::test]
async fn run_handle_has_unique_run_id() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run1");
    let h2 = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run2");
    assert_ne!(h1.run_id, h2.run_id);
    // Drain handles to avoid leaks.
    let _: Vec<_> = h1.events.collect().await;
    let _: Vec<_> = h2.events.collect().await;
    let _ = h1.receipt.await;
    let _ = h2.receipt.await;
}

#[tokio::test]
async fn run_handle_events_are_streamable() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run");
    let events: Vec<_> = handle.events.collect().await;
    assert!(!events.is_empty(), "should receive at least one event");
    let _ = handle.receipt.await;
}

// ===========================================================================
// 9. Config resolution — runtime defaults
// ===========================================================================

#[test]
fn runtime_emulation_default_is_none() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_projection_default_is_none() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_stream_pipeline_default_is_none() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

// ===========================================================================
// 10. Error propagation — unknown backend becomes RuntimeError
// ===========================================================================

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::new();
    let result = rt.run_streaming("no_such_backend", mock_work_order()).await;
    assert!(result.is_err());
    let err = result.map(|_| ()).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn error_code_mapping_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn error_code_mapping_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("oops"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn error_code_mapping_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn error_code_mapping_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn error_code_mapping_capability_check() {
    let err = RuntimeError::CapabilityCheckFailed("missing".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn is_retryable_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
}

#[test]
fn is_retryable_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
}

#[test]
fn is_not_retryable_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(!err.is_retryable());
}

#[test]
fn is_not_retryable_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("permanent"));
    assert!(!err.is_retryable());
}

#[test]
fn into_abp_error_preserves_code() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    let code = err.error_code();
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, code);
}

// ===========================================================================
// 11. Policy enforcement — policy globs compile during run
// ===========================================================================

#[tokio::test]
async fn policy_with_deny_read_compiles_and_runs() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into()],
        ..Default::default()
    };
    let (_, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn policy_with_allow_write_compiles_and_runs() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.policy = PolicyProfile {
        deny_write: vec!["**/*.lock".into()],
        ..Default::default()
    };
    let (_, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ===========================================================================
// 12. Event types through runtime — all AgentEventKind variants flow
// ===========================================================================

#[tokio::test]
async fn events_include_run_started_and_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_to_completion(&rt, mock_work_order()).await;
    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_started, "must include RunStarted");
    assert!(has_completed, "must include RunCompleted");
}

#[tokio::test]
async fn events_have_timestamps() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_to_completion(&rt, mock_work_order()).await;
    for ev in &events {
        // Timestamps should be non-zero / valid (year > 2020)
        assert!(ev.ts.timestamp() > 0, "event timestamp should be positive");
    }
}

#[test]
fn all_agent_event_kinds_constructable() {
    // Verify every variant can be constructed and serialized.
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        AgentEventKind::AssistantMessage { text: "msg".into() },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("ok"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Compiling...".into()),
        },
        AgentEventKind::Warning {
            message: "careful".into(),
        },
        AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let ev = make_event(kind);
        let json = serde_json::to_string(&ev).expect("serialize event");
        assert!(!json.is_empty());
    }
}

// ===========================================================================
// Additional: Telemetry, Hooks, Router, Cancellation
// ===========================================================================

#[test]
fn run_metrics_initial_snapshot_is_zero() {
    let metrics = RunMetrics::new();
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
}

#[test]
fn run_metrics_record_updates_snapshot() {
    let metrics = RunMetrics::new();
    metrics.record_run(100, true, 5);
    metrics.record_run(200, false, 3);
    let snap = metrics.snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 8);
}

#[tokio::test]
async fn runtime_metrics_updated_after_run() {
    let rt = Runtime::with_default_backends();
    let snap_before = rt.metrics().snapshot();
    assert_eq!(snap_before.total_runs, 0);
    let _ = run_to_completion(&rt, mock_work_order()).await;
    let snap_after = rt.metrics().snapshot();
    assert_eq!(snap_after.total_runs, 1);
    assert_eq!(snap_after.successful_runs, 1);
}

#[test]
fn hook_registry_fires_in_order() {
    struct NameHook(&'static str);
    impl LifecycleHook for NameHook {
        fn name(&self) -> &str {
            self.0
        }
    }
    let mut registry = HookRegistry::new();
    registry.register(Box::new(NameHook("first")));
    registry.register(Box::new(NameHook("second")));
    assert_eq!(registry.hook_count(), 2);
    assert_eq!(registry.hook_names(), vec!["first", "second"]);
}

#[test]
fn validation_hook_rejects_empty_task() {
    let hook = ValidationHook;
    let mut wo = mock_work_order();
    wo.task = "".into();
    let result = hook.on_run_start(&wo);
    assert!(result.is_err());
}

#[test]
fn validation_hook_accepts_valid_work_order() {
    let hook = ValidationHook;
    let wo = mock_work_order();
    let result = hook.on_run_start(&wo);
    assert!(result.is_ok());
}

#[test]
fn event_router_dispatches_by_kind() {
    let mut router = EventRouter::new();
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let c = Arc::clone(&counter);
    router.add_route(
        "run_started",
        Box::new(move |_ev| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    );
    router.route(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    // This should not match the "run_started" route.
    router.route(&make_event(AgentEventKind::Warning {
        message: "nope".into(),
    }));
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn event_router_route_count() {
    let mut router = EventRouter::new();
    assert_eq!(router.route_count(), 0);
    router.add_route("run_started", Box::new(|_| {}));
    router.add_route("warning", Box::new(|_| {}));
    assert_eq!(router.route_count(), 2);
}

#[test]
fn cancellation_token_starts_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_token_cancel_sets_flag() {
    let token = CancellationToken::new();
    let child = token.clone();
    token.cancel();
    assert!(child.is_cancelled());
}

#[test]
fn cancellable_run_default_no_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    assert!(!run.token().is_cancelled());
}

#[tokio::test]
async fn filtered_subscription_only_matching_events() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(&ev.kind, AgentEventKind::Warning { .. })),
    );
    bus.publish(make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    }));
    bus.publish(make_event(AgentEventKind::Warning {
        message: "b".into(),
    }));
    // Drop bus so the channel eventually closes after these events.
    drop(bus);
    let ev = filtered.recv().await;
    assert!(ev.is_some());
    assert!(matches!(ev.unwrap().kind, AgentEventKind::Warning { .. }));
}

#[tokio::test]
async fn receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    let _ = run_to_completion(&rt, mock_work_order()).await;
    let _ = run_to_completion(&rt, mock_work_order()).await;
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert!(guard.len() >= 2, "chain should have at least 2 receipts");
}

#[test]
fn backend_registry_list_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn logging_hook_name() {
    let hook = LoggingHook;
    assert_eq!(hook.name(), "logging");
}
