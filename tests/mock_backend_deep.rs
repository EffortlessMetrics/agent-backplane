#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Deep integration tests for MockBackend, Backend trait contract,
//! and BackendRegistry infrastructure.

use std::sync::Arc;

use abp_backend_core::{BackendHealth, BackendMetadata, BackendRegistry, HealthStatus, RateLimit};
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement,
    CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt, SupportLevel,
    WorkOrderBuilder,
};
use abp_integrations::Backend;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn simple_wo(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_collect(task: &str) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_wo(task), tx)
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

async fn run_receipt(task: &str) -> Receipt {
    let (tx, _rx) = mpsc::channel(64);
    MockBackend
        .run(Uuid::new_v4(), simple_wo(task), tx)
        .await
        .unwrap()
}

fn mock_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "0.1".to_string(),
        max_tokens: Some(4096),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

// ===========================================================================
// Section A – MockBackend behavior (10 tests)
// ===========================================================================

/// A01: MockBackend implements the Backend trait (compile-time + runtime check).
#[test]
fn a01_mock_implements_backend_trait() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let id = backend.identity();
    assert_eq!(id.id, "mock");
}

/// A02: Execute with a simple work order returns a receipt.
#[tokio::test]
async fn a02_execute_returns_receipt() {
    let receipt = run_receipt("hello world").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

/// A03: Execute with streaming produces events then receipt.
#[tokio::test]
async fn a03_execute_streams_events_then_receipt() {
    let (receipt, events) = run_collect("stream test").await;
    // Must have at least RunStarted and RunCompleted
    assert!(
        events.len() >= 2,
        "expected ≥2 events, got {}",
        events.len()
    );
    // First event is RunStarted
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted"
    );
    // Last event is RunCompleted
    assert!(
        matches!(
            &events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
    assert_eq!(receipt.outcome, Outcome::Complete);
}

/// A04: Mock responses reflect the task text in RunStarted message.
#[tokio::test]
async fn a04_mock_responses_reflect_task() {
    let (_, events) = run_collect("my custom task").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains("my custom task"),
            "RunStarted should mention the task"
        );
    } else {
        panic!("expected RunStarted");
    }
}

/// A05: Default mock produces AssistantMessage events (text deltas).
#[tokio::test]
async fn a05_default_mock_produces_assistant_messages() {
    let (_, events) = run_collect("delta test").await;
    let assistant_msgs: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .collect();
    assert!(
        !assistant_msgs.is_empty(),
        "mock should produce at least one AssistantMessage"
    );
}

/// A06: Receipt from mock has correct identity fields.
#[tokio::test]
async fn a06_receipt_correct_fields() {
    let receipt = run_receipt("fields test").await;
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert!(receipt.meta.duration_ms < 10_000, "should complete quickly");
}

/// A07: Receipt hash is valid (non-None and matches re-computation).
#[tokio::test]
async fn a07_receipt_hash_is_valid() {
    let receipt = run_receipt("hash test").await;
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt must have a hash after with_hash()"
    );
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert!(hash.len() == 64, "SHA-256 hex should be 64 chars");
    // Re-hash and verify determinism
    let rehashed = receipt.clone().with_hash().unwrap();
    assert_eq!(rehashed.receipt_sha256, receipt.receipt_sha256);
}

/// A08: MockBackend identity name is "mock".
#[test]
fn a08_mock_backend_name() {
    let id = MockBackend.identity();
    assert_eq!(id.id, "mock");
}

/// A09: MockBackend capabilities include streaming and tool capabilities.
#[test]
fn a09_mock_backend_capabilities() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
    assert!(caps.contains_key(&Capability::ToolRead));
    assert!(caps.contains_key(&Capability::ToolWrite));
    assert!(caps.contains_key(&Capability::ToolEdit));
    assert!(caps.contains_key(&Capability::ToolBash));
    // Streaming is native
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

/// A10: Concurrent mock executions don't interfere with each other.
#[tokio::test]
async fn a10_concurrent_mock_executions() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            let task = format!("concurrent-{i}");
            b.run(Uuid::new_v4(), simple_wo(&task), tx).await.unwrap()
        }));
    }
    let mut receipts = Vec::new();
    for h in handles {
        receipts.push(h.await.unwrap());
    }
    assert_eq!(receipts.len(), 5);
    // All should be Complete
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
    }
    // All hashes should be distinct (different run_ids and timestamps)
    let hashes: std::collections::HashSet<_> = receipts
        .iter()
        .filter_map(|r| r.receipt_sha256.clone())
        .collect();
    assert_eq!(hashes.len(), 5, "each receipt should have a unique hash");
}

// ===========================================================================
// Section B – Backend trait contract (10 tests)
// ===========================================================================

/// B01: Backend is object-safe (can be used as trait object).
#[test]
fn b01_backend_is_object_safe() {
    fn accepts_dyn(_b: &dyn Backend) {}
    accepts_dyn(&MockBackend);
}

/// B02: Backend name (identity.id) is non-empty.
#[test]
fn b02_backend_name_non_empty() {
    let id = MockBackend.identity();
    assert!(!id.id.is_empty(), "backend id must be non-empty");
}

/// B03: Backend capabilities include at least one entry.
#[test]
fn b03_backend_capabilities_non_empty() {
    let caps = MockBackend.capabilities();
    assert!(!caps.is_empty(), "capabilities must not be empty");
}

/// B04: Execute returns Result (never panics on valid input).
#[tokio::test]
async fn b04_execute_returns_result_not_panic() {
    let (tx, _rx) = mpsc::channel(64);
    let result = MockBackend.run(Uuid::new_v4(), simple_wo("safe"), tx).await;
    assert!(result.is_ok());
}

/// B05: Event channel is properly connected — all streamed events are received.
#[tokio::test]
async fn b05_event_channel_connected() {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_wo("channel test"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    // The trace in the receipt should match events sent
    assert_eq!(
        count,
        receipt.trace.len(),
        "channel events should match receipt trace length"
    );
}

/// B06: Receipt is always produced on success.
#[tokio::test]
async fn b06_receipt_always_produced() {
    for task in &["a", "b", "c", "", "long task description here"] {
        let (tx, _rx) = mpsc::channel(64);
        let result = MockBackend.run(Uuid::new_v4(), simple_wo(task), tx).await;
        assert!(result.is_ok(), "receipt must be produced for task '{task}'");
    }
}

/// B07: Backend handles empty work order task.
#[tokio::test]
async fn b07_handles_empty_task() {
    let receipt = run_receipt("").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

/// B08: Backend handles very long task descriptions.
#[tokio::test]
async fn b08_handles_long_task() {
    let long_task = "x".repeat(100_000);
    let receipt = run_receipt(&long_task).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

/// B09: Backend respects capability requirements — fails when unsatisfied.
#[tokio::test]
async fn b09_capability_requirements_enforced() {
    let mut wo = simple_wo("cap test");
    // Require a capability that mock doesn't have at Native level but only Emulated
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(64);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    // ToolRead is Emulated in mock, requiring Native should fail
    assert!(result.is_err(), "should fail when Native ToolRead required");
}

/// B10: Backend cleanup after execution — channel can be dropped.
#[tokio::test]
async fn b10_cleanup_after_execution() {
    let (tx, rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_wo("cleanup"), tx)
        .await
        .unwrap();
    // Drop receiver — should not panic
    drop(rx);
}

// ===========================================================================
// Section C – Backend registry integration (10 tests)
// ===========================================================================

/// C01: Register MockBackend metadata in registry.
#[test]
fn c01_register_mock_in_registry() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", mock_metadata("mock", "abp"));
    assert!(reg.contains("mock"));
}

/// C02: Retrieve backend metadata by name.
#[test]
fn c02_retrieve_by_name() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", mock_metadata("mock", "abp"));
    let meta = reg.metadata("mock").unwrap();
    assert_eq!(meta.name, "mock");
    assert_eq!(meta.dialect, "abp");
}

/// C03: List all registered backends.
#[test]
fn c03_list_all_registered() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("alpha", mock_metadata("alpha", "openai"));
    reg.register_with_metadata("beta", mock_metadata("beta", "anthropic"));
    let names = reg.list();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

/// C04: Remove backend from registry.
#[test]
fn c04_remove_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", mock_metadata("mock", "abp"));
    assert!(reg.contains("mock"));
    let removed = reg.remove("mock");
    assert!(removed.is_some());
    assert!(!reg.contains("mock"));
    assert_eq!(reg.len(), 0);
}

/// C05: Backend with full metadata registration.
#[test]
fn c05_metadata_registration() {
    let mut reg = BackendRegistry::new();
    let meta = BackendMetadata {
        name: "gpt-4".to_string(),
        dialect: "openai".to_string(),
        version: "2024-01".to_string(),
        max_tokens: Some(128_000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: Some(RateLimit {
            requests_per_minute: 60,
            tokens_per_minute: 150_000,
            concurrent_requests: 10,
        }),
    };
    reg.register_with_metadata("gpt-4", meta);
    let stored = reg.metadata("gpt-4").unwrap();
    assert_eq!(stored.max_tokens, Some(128_000));
    assert!(stored.rate_limit.is_some());
    let rl = stored.rate_limit.as_ref().unwrap();
    assert_eq!(rl.requests_per_minute, 60);
}

/// C06: Health tracking for mock backend.
#[test]
fn c06_health_tracking() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", mock_metadata("mock", "abp"));
    // Default health is Unknown
    let h = reg.health("mock").unwrap();
    assert_eq!(h.status, HealthStatus::Unknown);
    // Update to Healthy
    reg.update_health(
        "mock",
        BackendHealth {
            status: HealthStatus::Healthy,
            last_check: Some(chrono::Utc::now()),
            latency_ms: Some(42),
            error_rate: 0.0,
            consecutive_failures: 0,
        },
    );
    let h = reg.health("mock").unwrap();
    assert_eq!(h.status, HealthStatus::Healthy);
    assert_eq!(h.latency_ms, Some(42));
}

/// C07: Multiple backends registered simultaneously.
#[test]
fn c07_multiple_backends() {
    let mut reg = BackendRegistry::new();
    let dialects = ["openai", "anthropic", "gemini", "mock"];
    for d in &dialects {
        reg.register_with_metadata(d, mock_metadata(d, d));
    }
    assert_eq!(reg.len(), 4);
    let list = reg.list();
    for d in &dialects {
        assert!(list.contains(d), "missing {d}");
    }
}

/// C08: Backend selection by dialect.
#[test]
fn c08_selection_by_dialect() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt", mock_metadata("gpt", "openai"));
    reg.register_with_metadata("claude", mock_metadata("claude", "anthropic"));
    reg.register_with_metadata("gpt-mini", mock_metadata("gpt-mini", "openai"));
    let openai_backends = reg.by_dialect("openai");
    assert_eq!(openai_backends.len(), 2);
    assert!(openai_backends.contains(&"gpt"));
    assert!(openai_backends.contains(&"gpt-mini"));
    let anthropic = reg.by_dialect("anthropic");
    assert_eq!(anthropic.len(), 1);
    assert!(anthropic.contains(&"claude"));
}

/// C09: Registry survives backend failure (unhealthy status).
#[test]
fn c09_registry_survives_failure() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", mock_metadata("mock", "abp"));
    reg.update_health(
        "mock",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            last_check: Some(chrono::Utc::now()),
            latency_ms: None,
            error_rate: 1.0,
            consecutive_failures: 5,
        },
    );
    // Backend still exists in registry
    assert!(reg.contains("mock"));
    assert_eq!(reg.len(), 1);
    // But not in healthy list
    assert!(reg.healthy_backends().is_empty());
    // Can still retrieve metadata
    assert!(reg.metadata("mock").is_some());
    // Re-mark healthy
    reg.update_health(
        "mock",
        BackendHealth {
            status: HealthStatus::Healthy,
            last_check: Some(chrono::Utc::now()),
            latency_ms: Some(10),
            error_rate: 0.0,
            consecutive_failures: 0,
        },
    );
    assert_eq!(reg.healthy_backends().len(), 1);
}

/// C10: Empty registry behavior.
#[test]
fn c10_empty_registry() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.list().is_empty());
    assert!(reg.healthy_backends().is_empty());
    assert!(reg.metadata("nonexistent").is_none());
    assert!(reg.health("nonexistent").is_none());
    assert!(!reg.contains("anything"));
}
