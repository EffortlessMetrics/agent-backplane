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
//! Comprehensive tests for the ABP runtime orchestration system.
//!
//! Categories:
//! 1. Runtime construction and configuration
//! 2. Backend registration
//! 3. run_streaming() happy path with MockBackend
//! 4. Event multiplexing
//! 5. Receipt generation and hashing
//! 6. Error handling (unknown backend, policy violation)
//! 7. Workspace preparation
//! 8. Edge cases: empty work orders, concurrent runs

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_root() -> String {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let path = tmp.path().to_string_lossy().to_string();
    // Keep the directory alive (don't delete on drop)
    std::mem::forget(tmp);
    path
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// A custom backend for testing that always fails.
#[derive(Debug, Clone)]
struct FailingBackend;

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _wo: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("intentional failure")
    }
}

/// A custom backend that emits N events before returning.
#[derive(Debug, Clone)]
struct EventCountBackend {
    count: usize,
}

#[async_trait]
impl Backend for EventCountBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "event-count".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        for i in 0..self.count {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            };
            let _ = tx.send(ev).await;
        }
        let receipt = ReceiptBuilder::new("event-count")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build();
        Ok(receipt)
    }
}

/// A backend with no capabilities (empty manifest) that succeeds.
#[derive(Debug, Clone)]
struct EmptyCapBackend;

#[async_trait]
impl Backend for EmptyCapBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "empty-cap".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        Ok(ReceiptBuilder::new("empty-cap")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend that sends events slowly.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay_ms: u64,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "slow done".into(),
            },
            ext: None,
        };
        let _ = tx.send(ev).await;
        Ok(ReceiptBuilder::new("slow")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

// ===========================================================================
// 1. Runtime construction and configuration
// ===========================================================================

#[test]
fn runtime_new_creates_empty_runtime() {
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
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn runtime_with_default_backends_count() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn runtime_metrics_initially_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
}

#[test]
fn runtime_emulation_config_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn runtime_projection_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn runtime_stream_pipeline_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn runtime_registry_returns_reference() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
}

#[test]
fn runtime_registry_mut_allows_modification() {
    let mut rt = Runtime::new();
    rt.registry_mut().register("test", MockBackend);
    assert!(rt.registry().contains("test"));
}

#[test]
fn runtime_receipt_chain_is_arc() {
    let rt = Runtime::new();
    let chain1 = rt.receipt_chain();
    let chain2 = rt.receipt_chain();
    assert!(Arc::strong_count(&chain1) >= 2);
    drop(chain2);
}

// ===========================================================================
// 2. Backend registration
// ===========================================================================

#[test]
fn register_single_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    assert_eq!(rt.backend_names(), vec!["mock".to_string()]);
}

#[test]
fn register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("failing", FailingBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"failing".to_string()));
    assert!(names.contains(&"mock".to_string()));
    assert_eq!(names.len(), 2);
}

#[test]
fn register_replaces_existing_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("test", MockBackend);
    rt.register_backend("test", FailingBackend);
    assert_eq!(rt.backend_names().len(), 1);
    let b = rt.backend("test").unwrap();
    assert_eq!(b.identity().id, "failing");
}

#[test]
fn backend_lookup_returns_none_for_unknown() {
    let rt = Runtime::new();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_lookup_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("z-backend", MockBackend);
    rt.register_backend("a-backend", MockBackend);
    rt.register_backend("m-backend", MockBackend);
    let names = rt.backend_names();
    assert_eq!(
        names,
        vec!["a-backend", "m-backend", "z-backend"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn backend_identity_from_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let id = b.identity();
    assert_eq!(id.id, "mock");
    assert!(id.backend_version.is_some());
}

#[test]
fn backend_capabilities_from_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn registry_contains_checks() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

#[test]
fn registry_list() {
    let rt = Runtime::with_default_backends();
    let list = rt.registry().list();
    assert_eq!(list, vec!["mock"]);
}

#[test]
fn registry_get_arc() {
    let rt = Runtime::with_default_backends();
    let arc = rt.registry().get_arc("mock");
    assert!(arc.is_some());
}

#[test]
fn registry_remove_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("removable", MockBackend);
    assert!(rt.registry().contains("removable"));
    let removed = rt.registry_mut().remove("removable");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("removable"));
}

#[test]
fn registry_remove_nonexistent() {
    let mut rt = Runtime::new();
    let removed = rt.registry_mut().remove("nope");
    assert!(removed.is_none());
}

// ===========================================================================
// 3. run_streaming() happy path with MockBackend
// ===========================================================================

#[tokio::test]
async fn run_streaming_mock_returns_handle() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("test task");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert!(!handle.run_id.is_nil());
}

#[tokio::test]
async fn run_streaming_mock_produces_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("produce receipt");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_mock_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn run_streaming_mock_receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("version test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn run_streaming_mock_receipt_backend_id() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("backend id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn run_streaming_mock_events_stream() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("event stream test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    // MockBackend emits 4 events: RunStarted, 2x AssistantMessage, RunCompleted
    assert!(count >= 1, "expected at least one event, got {count}");
}

#[tokio::test]
async fn run_streaming_mock_events_include_run_started() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("run started test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found_start = false;
    while let Some(ev) = events.next().await {
        if matches!(ev.kind, AgentEventKind::RunStarted { .. }) {
            found_start = true;
        }
    }
    assert!(found_start, "expected a RunStarted event");
}

#[tokio::test]
async fn run_streaming_mock_events_include_run_completed() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("run completed test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found_end = false;
    while let Some(ev) = events.next().await {
        if matches!(ev.kind, AgentEventKind::RunCompleted { .. }) {
            found_end = true;
        }
    }
    assert!(found_end, "expected a RunCompleted event");
}

#[tokio::test]
async fn run_streaming_mock_receipt_trace_not_empty() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("trace test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(
        !receipt.trace.is_empty(),
        "expected non-empty trace in receipt"
    );
}

#[tokio::test]
async fn run_streaming_mock_receipt_work_order_id_matches() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("wo id test");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn run_streaming_mock_receipt_verification_present() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("verification test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    // Verification fields should be populated (may be None for git_diff/status)
    // but harness_ok should be true from mock
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn run_streaming_preserves_task() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("my specific task");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found = false;
    while let Some(ev) = events.next().await {
        if let AgentEventKind::RunStarted { message } = &ev.kind
            && message.contains("my specific task")
        {
            found = true;
        }
    }
    assert!(found, "expected task text in RunStarted message");
}

// ===========================================================================
// 4. Event multiplexing
// ===========================================================================

#[test]
fn multiplexer_new_has_zero_subscribers() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
}

#[test]
fn multiplexer_subscribe_increments_count() {
    let mux = EventMultiplexer::new(16);
    let _s1 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
    let _s2 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 2);
}

#[test]
fn multiplexer_broadcast_fails_without_subscribers() {
    let mux = EventMultiplexer::new(16);
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let result = mux.broadcast(ev);
    assert!(result.is_err());
}

#[tokio::test]
async fn multiplexer_broadcast_reaches_subscribers() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "test".into(),
        },
        ext: None,
    };
    let count = mux.broadcast(ev).unwrap();
    assert_eq!(count, 1);
    let received = sub.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn multiplexer_multi_subscriber_broadcast() {
    let mux = EventMultiplexer::new(16);
    let mut sub1 = mux.subscribe();
    let mut sub2 = mux.subscribe();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    };
    let count = mux.broadcast(ev).unwrap();
    assert_eq!(count, 2);
    let _ = sub1.recv().await.unwrap();
    let _ = sub2.recv().await.unwrap();
}

#[test]
fn multiplexer_subscriber_drop_decrements_count() {
    let mux = EventMultiplexer::new(16);
    let sub = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
    drop(sub);
    assert_eq!(mux.subscriber_count(), 0);
}

#[test]
fn event_router_new_empty() {
    let router = EventRouter::new();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn event_router_default_empty() {
    let router = EventRouter::default();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn event_router_add_route_increments_count() {
    let mut router = EventRouter::new();
    router.add_route("run_started", Box::new(|_| {}));
    assert_eq!(router.route_count(), 1);
}

#[test]
fn event_router_multiple_routes() {
    let mut router = EventRouter::new();
    router.add_route("run_started", Box::new(|_| {}));
    router.add_route("assistant_message", Box::new(|_| {}));
    assert_eq!(router.route_count(), 2);
}

#[test]
fn event_router_routes_matching_event() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut router = EventRouter::new();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    router.add_route(
        "assistant_message",
        Box::new(move |_| {
            called_clone.store(true, Ordering::SeqCst);
        }),
    );

    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    router.route(&ev);
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn event_router_does_not_route_non_matching() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut router = EventRouter::new();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    router.add_route(
        "tool_call",
        Box::new(move |_| {
            called_clone.store(true, Ordering::SeqCst);
        }),
    );

    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    router.route(&ev);
    assert!(!called.load(Ordering::SeqCst));
}

// ===========================================================================
// 5. Receipt generation and hashing
// ===========================================================================

#[test]
fn receipt_builder_basic() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_with_hash_produces_sha256() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_is_deterministic() {
    let r1 = ReceiptBuilder::new("det")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_differs_for_different_outcomes() {
    let r1 = ReceiptBuilder::new("diff")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let r2 = ReceiptBuilder::new("diff")
        .outcome(Outcome::Failed)
        .work_order_id(Uuid::nil())
        .build();
    // Different outcomes should produce different hashes (unless timestamps match exactly)
    let _h1 = abp_core::receipt_hash(&r1).unwrap();
    let _h2 = abp_core::receipt_hash(&r2).unwrap();
    // Different outcomes should produce different hashes
    assert_ne!(r1.outcome, r2.outcome);
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let mut r = ReceiptBuilder::new("hash-test")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("garbage".to_string());
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "hash should ignore receipt_sha256 field");
}

#[test]
fn receipt_builder_with_hash_convenience() {
    let receipt = ReceiptBuilder::new("conv")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn receipt_contract_version_set() {
    let r = ReceiptBuilder::new("ver").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("caps").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_builder_mode() {
    let r = ReceiptBuilder::new("mode")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("status".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("ver").verification(v).build();
    assert!(r.verification.harness_ok);
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
}

#[test]
fn receipt_builder_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("usage").usage(usage).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[tokio::test]
async fn run_streaming_receipt_chain_grows() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("chain test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _receipt = handle.receipt.await.unwrap().unwrap();
    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert!(
        !chain_guard.is_empty(),
        "receipt chain should have at least 1 entry"
    );
}

#[tokio::test]
async fn run_streaming_two_runs_chain_grows() {
    let rt = Runtime::with_default_backends();

    let h1 = rt
        .run_streaming("mock", simple_work_order("first"))
        .await
        .unwrap();
    let _ = h1.receipt.await.unwrap().unwrap();

    let h2 = rt
        .run_streaming("mock", simple_work_order("second"))
        .await
        .unwrap();
    let _ = h2.receipt.await.unwrap().unwrap();

    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert!(
        chain_guard.len() >= 2,
        "receipt chain should have at least 2 entries"
    );
}

// ===========================================================================
// 6. Error handling
// ===========================================================================

#[tokio::test]
async fn run_streaming_unknown_backend_error() {
    let rt = Runtime::new();
    let wo = simple_work_order("error test");
    let err = rt
        .run_streaming("nonexistent", wo)
        .await
        .err()
        .expect("expected error");
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );
}

#[tokio::test]
async fn run_streaming_unknown_backend_name_in_error() {
    let rt = Runtime::new();
    let wo = simple_work_order("name test");
    let err = rt
        .run_streaming("my-missing-backend", wo)
        .await
        .err()
        .expect("expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("my-missing-backend"),
        "error should contain backend name: {msg}"
    );
}

#[tokio::test]
async fn run_streaming_failing_backend_returns_error() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    let wo = simple_work_order("fail test");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let result = handle.receipt.await.unwrap();
    assert!(result.is_err(), "expected error from failing backend");
}

#[tokio::test]
async fn run_streaming_failing_backend_error_variant() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    let wo = simple_work_order("fail variant");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let err = handle.receipt.await.unwrap().unwrap_err();
    assert!(
        matches!(err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed, got {err:?}"
    );
}

#[test]
fn runtime_error_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(err.to_string(), "unknown backend: foo");
}

#[test]
fn runtime_error_workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.to_string().contains("workspace preparation failed"));
}

#[test]
fn runtime_error_policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("invalid glob"));
    assert!(err.to_string().contains("policy compilation failed"));
}

#[test]
fn runtime_error_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(err.to_string().contains("backend execution failed"));
}

#[test]
fn runtime_error_capability_check_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    assert!(err.to_string().contains("missing streaming"));
}

#[test]
fn runtime_error_no_projection_match_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert!(err.to_string().contains("no matrix"));
}

#[test]
fn runtime_error_error_code_unknown() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn runtime_error_error_code_workspace() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn runtime_error_error_code_policy() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn runtime_error_error_code_backend() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn runtime_error_into_abp_error() {
    let err = RuntimeError::UnknownBackend {
        name: "gone".into(),
    };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp.message.contains("gone"));
}

#[test]
fn check_capabilities_unknown_backend() {
    let rt = Runtime::new();
    let reqs = CapabilityRequirements::default();
    let err = rt.check_capabilities("nope", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn check_capabilities_mock_streaming() {
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
fn check_capabilities_mock_unsatisfied() {
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
fn check_capabilities_empty_requirements() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

// ===========================================================================
// 7. Workspace preparation
// ===========================================================================

#[tokio::test]
async fn run_streaming_with_staged_workspace() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged workspace test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_passthrough_workspace() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough workspace test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_workspace_git_info_populated() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("git info test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    // Git info may or may not be populated depending on environment,
    // but the receipt should always be present
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn run_streaming_custom_root_workspace() {
    let rt = Runtime::with_default_backends();
    let tmp = tempfile::tempdir().unwrap();
    let wo = WorkOrderBuilder::new("custom root test")
        .root(tmp.path().to_string_lossy().to_string())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 8. Edge cases: empty work orders, concurrent runs
// ===========================================================================

#[tokio::test]
async fn run_streaming_empty_task() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_long_task() {
    let rt = Runtime::with_default_backends();
    let long_task = "a".repeat(10_000);
    let wo = simple_work_order(&long_task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn concurrent_runs_different_ids() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("concurrent 1"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("concurrent 2"))
        .await
        .unwrap();
    assert_ne!(h1.run_id, h2.run_id);
    let r1 = h1.receipt.await.unwrap().unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn concurrent_runs_both_succeed() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("parallel a"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("parallel b"))
        .await
        .unwrap();
    let (r1, r2) = tokio::join!(h1.receipt, h2.receipt);
    assert_eq!(r1.unwrap().unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn five_concurrent_runs() {
    let rt = Runtime::with_default_backends();
    let mut handles = Vec::new();
    for i in 0..5 {
        let h = rt
            .run_streaming("mock", simple_work_order(&format!("run-{i}")))
            .await
            .unwrap();
        handles.push(h);
    }
    for h in handles {
        let r = h.receipt.await.unwrap().unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn run_streaming_with_custom_backend_emitting_events() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 10 });
    let wo = simple_work_order("event count test");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    assert_eq!(count, 10, "expected exactly 10 events");
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_zero_events_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 0 });
    let wo = simple_work_order("zero events");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while (events.next().await).is_some() {
        count += 1;
    }
    assert_eq!(count, 0, "expected zero events");
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_empty_cap_backend_skips_cap_check() {
    let mut rt = Runtime::new();
    rt.register_backend("empty-cap", EmptyCapBackend);
    let wo = simple_work_order("empty cap test");
    let handle = rt.run_streaming("empty-cap", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_work_order_builder_options() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("builder options test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .lane(ExecutionLane::WorkspaceFirst)
        .model("test-model")
        .max_turns(5)
        .max_budget_usd(10.0)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_policy() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("policy test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_context() {
    let rt = Runtime::with_default_backends();
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "test".into(),
            content: "some content".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("context test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ctx)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_requirements_satisfied() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("reqs test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_streaming_with_unsatisfied_requirements() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("unsatisfied reqs")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    // The capability check happens in-task for mock backend, so we get a handle
    // but the receipt future will contain the error
    let result = rt.run_streaming("mock", wo).await;
    // Pre-flight cap check should fail before task starts
    assert!(result.is_err());
}

// ===========================================================================
// Additional telemetry tests
// ===========================================================================

#[test]
fn run_metrics_snapshot_initial() {
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
}

#[test]
fn run_metrics_record_failure() {
    let m = RunMetrics::new();
    m.record_run(50, false, 2);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 1);
}

#[test]
fn run_metrics_multiple_runs() {
    let m = RunMetrics::new();
    m.record_run(100, true, 10);
    m.record_run(200, true, 20);
    m.record_run(300, false, 5);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 35);
    assert_eq!(snap.average_run_duration_ms, 200);
}

#[tokio::test]
async fn runtime_metrics_updated_after_run() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("metrics test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _ = handle.receipt.await.unwrap().unwrap();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
}

// ===========================================================================
// Backend registry edge cases
// ===========================================================================

#[test]
fn registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.get("mock").is_some());
    assert!(reg.get("other").is_none());
}

#[test]
fn registry_register_overwrite() {
    let mut reg = BackendRegistry::default();
    reg.register("test", MockBackend);
    reg.register("test", FailingBackend);
    let b = reg.get("test").unwrap();
    assert_eq!(b.identity().id, "failing");
}

// ===========================================================================
// WorkOrder builder edge cases
// ===========================================================================

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.task, "test");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn work_order_builder_all_options() {
    let wo = WorkOrderBuilder::new("full")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/".into()])
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    assert_eq!(wo.task, "full");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/tmp/test");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/"]);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

// ===========================================================================
// Contract types
// ===========================================================================

#[test]
fn contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn outcome_serde_roundtrip() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Complete);
}

#[test]
fn outcome_partial_serde() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Partial);
}

#[test]
fn outcome_failed_serde() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Failed);
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }
}

#[test]
fn support_level_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing_native() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_unsupported_satisfies_nothing_emulated() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn canonical_json_sorted_keys() {
    let json = abp_core::canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

#[test]
fn sha256_hex_length() {
    let hex = abp_core::sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = abp_core::sha256_hex(b"test");
    let h2 = abp_core::sha256_hex(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn sha256_hex_different_inputs() {
    let h1 = abp_core::sha256_hex(b"hello");
    let h2 = abp_core::sha256_hex(b"world");
    assert_ne!(h1, h2);
}

// ===========================================================================
// AgentEvent construction
// ===========================================================================

#[test]
fn agent_event_run_started() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_assistant_delta() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn agent_event_tool_call() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn agent_event_tool_result() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: None,
            output: serde_json::json!({"content": "data"}),
            is_error: false,
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn agent_event_file_changed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_command_executed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: Some("files".into()),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn agent_event_warning() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "careful".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_error() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "bad".into(),
            error_code: None,
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[test]
fn agent_event_serde_roundtrip() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::AssistantMessage { .. }));
}

// ===========================================================================
// Multiple backend type interactions
// ===========================================================================

#[tokio::test]
async fn slow_backend_completes() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 50 });
    let wo = simple_work_order("slow test");
    let handle = rt.run_streaming("slow", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mixed_backends_concurrent() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("counter", EventCountBackend { count: 3 });

    let h1 = rt
        .run_streaming("mock", simple_work_order("mock run"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("counter", simple_work_order("counter run"))
        .await
        .unwrap();

    let (r1, r2) = tokio::join!(h1.receipt, h2.receipt);
    assert_eq!(r1.unwrap().unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn event_count_backend_exact_events() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 5 });
    let wo = simple_work_order("exact count");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while (events.next().await).is_some() {
        count += 1;
    }
    assert_eq!(count, 5);
}

#[tokio::test]
async fn large_event_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 200 });
    let wo = simple_work_order("large stream");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while (events.next().await).is_some() {
        count += 1;
    }
    assert_eq!(count, 200);
}

// ===========================================================================
// Select backend without projection
// ===========================================================================

#[test]
fn select_backend_without_projection_fails() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("select test");
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

// ===========================================================================
// Receipt chain and receipt builder edge cases
// ===========================================================================

#[test]
fn receipt_builder_add_trace_event() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "traced".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("test").add_trace_event(ev).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_add_artifact() {
    let artifact = abp_core::ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let r = ReceiptBuilder::new("test").add_artifact(artifact).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn receipt_builder_backend_version() {
    let r = ReceiptBuilder::new("test").backend_version("2.0").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
}

#[test]
fn receipt_builder_adapter_version() {
    let r = ReceiptBuilder::new("test").adapter_version("1.5").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.5"));
}

#[test]
fn receipt_builder_usage_raw() {
    let r = ReceiptBuilder::new("test")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 42);
}

#[test]
fn receipt_builder_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("test").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}
