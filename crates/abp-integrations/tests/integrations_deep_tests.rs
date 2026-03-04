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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the abp-integrations crate covering:
//! Backend trait, MockBackend, SidecarBackend, BackendRegistry, vendor config,
//! mode detection, event streaming, receipts, errors, capabilities, serde, and more.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_host::SidecarSpec;
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::health::{HealthChecker, HealthStatus};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry, MetricsSnapshot};
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use abp_integrations::{
    Backend, MockBackend, SidecarBackend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn work_order_with_vendor(key: &str, value: serde_json::Value) -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert(key.to_string(), value);
    WorkOrderBuilder::new("test")
        .config(abp_core::RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build()
}

/// Minimal custom backend for testing trait object safety.
#[derive(Debug, Clone)]
struct StubBackend {
    name: String,
    caps: CapabilityManifest,
}

impl StubBackend {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            caps: CapabilityManifest::new(),
        }
    }

    fn with_cap(mut self, cap: Capability, level: SupportLevel) -> Self {
        self.caps.insert(cap, level);
        self
    }
}

#[async_trait]
impl Backend for StubBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("stub-0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.caps.clone()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<abp_core::Receipt> {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("stub: {}", work_order.task),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        let mut receipt = ReceiptBuilder::new(&self.name).build().with_hash()?;
        receipt.meta.run_id = run_id;
        receipt.meta.work_order_id = work_order.id;
        Ok(receipt)
    }
}

/// A backend that always fails.
#[derive(Debug)]
struct ErrorBackend {
    message: String,
}

#[async_trait]
impl Backend for ErrorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error".into(),
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
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<abp_core::Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

// ===========================================================================
// 1. Backend trait: trait exists, methods callable
// ===========================================================================

#[tokio::test]
async fn backend_trait_identity_callable() {
    let b = MockBackend;
    let id = b.identity();
    assert_eq!(id.id, "mock");
}

#[tokio::test]
async fn backend_trait_capabilities_callable() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(!caps.is_empty());
}

#[tokio::test]
async fn backend_trait_run_callable() {
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend
        .run(Uuid::new_v4(), simple_work_order("hello"), tx)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn backend_trait_object_safety() {
    // Backend can be used as a trait object.
    let b: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(b.identity().id, "mock");
}

// ===========================================================================
// 2. MockBackend: construction, name, run produces receipt with events
// ===========================================================================

#[tokio::test]
async fn mock_backend_name_is_mock() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[tokio::test]
async fn mock_backend_has_backend_version() {
    let id = MockBackend.identity();
    assert!(id.backend_version.is_some());
}

#[tokio::test]
async fn mock_run_produces_receipt_with_events() {
    let (tx, mut rx) = mpsc::channel(32);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("task"), tx)
        .await
        .unwrap();

    // Receipt has a hash
    assert!(receipt.receipt_sha256.is_some());

    // Events were streamed
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 2, "expected at least 2 events, got {count}");
}

#[tokio::test]
async fn mock_receipt_outcome_is_complete() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_receipt_contract_version_set() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(!receipt.meta.contract_version.is_empty());
}

// ===========================================================================
// 3. SidecarBackend: construction, name matches
// ===========================================================================

#[test]
fn sidecar_backend_construction_from_spec() {
    let spec = SidecarSpec::new("node");
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.command, "node");
}

#[test]
fn sidecar_backend_identity_is_sidecar() {
    let spec = SidecarSpec {
        command: "python".into(),
        args: vec!["host.py".into()],
        env: BTreeMap::new(),
        cwd: Some("/tmp".into()),
    };
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.identity().id, "sidecar");
}

#[test]
fn sidecar_backend_capabilities_empty_by_default() {
    let sb = SidecarBackend::new(SidecarSpec::new("echo"));
    assert!(sb.capabilities().is_empty());
}

#[test]
fn sidecar_backend_spec_preserved() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "VAL".into());
    let spec = SidecarSpec {
        command: "cargo".into(),
        args: vec!["run".into()],
        env,
        cwd: Some("/home".into()),
    };
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.args, vec!["run"]);
    assert_eq!(sb.spec.env.get("KEY").unwrap(), "VAL");
    assert_eq!(sb.spec.cwd.as_deref(), Some("/home"));
}

// ===========================================================================
// 4. Backend registry: add/get/list, duplicate name handling
// ===========================================================================

#[test]
fn registry_empty_initially() {
    let reg = abp_backend_core::BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_register_and_list() {
    let mut reg = abp_backend_core::BackendRegistry::new();
    reg.register_with_metadata(
        "alpha",
        abp_backend_core::BackendMetadata {
            name: "alpha".into(),
            dialect: "openai".into(),
            version: "1.0".into(),
            max_tokens: Some(4096),
            supports_streaming: true,
            supports_tools: true,
            rate_limit: None,
        },
    );
    assert!(reg.contains("alpha"));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.list(), vec!["alpha"]);
}

#[test]
fn registry_duplicate_name_overwrites() {
    let mut reg = abp_backend_core::BackendRegistry::new();
    let meta1 = abp_backend_core::BackendMetadata {
        name: "dup".into(),
        dialect: "v1".into(),
        version: "1".into(),
        max_tokens: None,
        supports_streaming: false,
        supports_tools: false,
        rate_limit: None,
    };
    let meta2 = abp_backend_core::BackendMetadata {
        name: "dup".into(),
        dialect: "v2".into(),
        version: "2".into(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    };
    reg.register_with_metadata("dup", meta1);
    reg.register_with_metadata("dup", meta2);
    assert_eq!(reg.len(), 1);
    let m = reg.metadata("dup").unwrap();
    assert_eq!(m.dialect, "v2");
}

#[test]
fn registry_remove_backend() {
    let mut reg = abp_backend_core::BackendRegistry::new();
    reg.register_with_metadata(
        "rm",
        abp_backend_core::BackendMetadata {
            name: "rm".into(),
            dialect: "test".into(),
            version: "1".into(),
            max_tokens: None,
            supports_streaming: false,
            supports_tools: false,
            rate_limit: None,
        },
    );
    assert!(reg.contains("rm"));
    let removed = reg.remove("rm");
    assert!(removed.is_some());
    assert!(!reg.contains("rm"));
}

#[test]
fn registry_get_nonexistent_returns_none() {
    let reg = abp_backend_core::BackendRegistry::new();
    assert!(reg.metadata("nope").is_none());
    assert!(reg.health("nope").is_none());
}

// ===========================================================================
// 5. Vendor config parsing: "abp.mode", nested abp object
// ===========================================================================

#[test]
fn extract_mode_default_is_mapped() {
    let wo = simple_work_order("t");
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn extract_mode_flat_key_passthrough() {
    let wo = work_order_with_vendor("abp.mode", serde_json::json!("passthrough"));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_nested_abp_object() {
    let wo = work_order_with_vendor("abp", serde_json::json!({"mode": "passthrough"}));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_mapped_explicit() {
    let wo = work_order_with_vendor("abp.mode", serde_json::json!("mapped"));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn extract_mode_invalid_falls_back_to_default() {
    let wo = work_order_with_vendor("abp.mode", serde_json::json!("bogus"));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

// ===========================================================================
// 6. Mode detection: Passthrough vs Mapped from config
// ===========================================================================

#[test]
fn passthrough_validation_succeeds() {
    let wo = simple_work_order("t");
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    let modes = [ExecutionMode::Passthrough, ExecutionMode::Mapped];
    for mode in &modes {
        let json = serde_json::to_string(mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, mode);
    }
}

// ===========================================================================
// 7. Event streaming: backend sends events through channel
// ===========================================================================

#[tokio::test]
async fn mock_streams_run_started_and_completed() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // First event should be RunStarted, last should be RunCompleted
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn stub_backend_streams_event() {
    let stub = StubBackend::new("s1").with_cap(Capability::Streaming, SupportLevel::Native);
    let (tx, mut rx) = mpsc::channel(16);
    let _receipt = stub
        .run(Uuid::new_v4(), simple_work_order("go"), tx)
        .await
        .unwrap();

    let ev = rx.try_recv().unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn events_carry_timestamps() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();

    while let Ok(ev) = rx.try_recv() {
        // All events should have a recent timestamp
        assert!(ev.ts.timestamp() > 0);
    }
}

// ===========================================================================
// 8. Receipt from backend: valid receipt with hash
// ===========================================================================

#[tokio::test]
async fn receipt_has_sha256_hash() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert!(!hash.is_empty());
}

#[tokio::test]
async fn receipt_run_id_matches_input() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(run_id, simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn receipt_work_order_id_matches() {
    let wo = simple_work_order("t");
    let wo_id = wo.id;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn receipt_trace_is_nonempty() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_serializable() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains("receipt_sha256"));
}

// ===========================================================================
// 9. Error handling: backend errors (connection, timeout, protocol)
// ===========================================================================

#[tokio::test]
async fn error_backend_returns_err() {
    let b = ErrorBackend {
        message: "connection refused".into(),
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = b.run(Uuid::new_v4(), simple_work_order("t"), tx).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("connection refused"));
}

#[tokio::test]
async fn error_backend_timeout_message() {
    let b = ErrorBackend {
        message: "timeout after 30s".into(),
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = b.run(Uuid::new_v4(), simple_work_order("t"), tx).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timeout"));
}

#[tokio::test]
async fn error_backend_protocol_error() {
    let b = ErrorBackend {
        message: "protocol error: invalid envelope".into(),
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = b.run(Uuid::new_v4(), simple_work_order("t"), tx).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("protocol error"));
}

#[test]
fn ensure_capabilities_fails_on_missing() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let caps = CapabilityManifest::new();
    let result = ensure_capability_requirements(&reqs, &caps);
    assert!(result.is_err());
}

#[test]
fn ensure_capabilities_passes_when_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = ensure_capability_requirements(&reqs, &MockBackend.capabilities());
    assert!(result.is_ok());
}

// ===========================================================================
// 10. Capability reporting: backend reports supported capabilities
// ===========================================================================

#[test]
fn mock_capabilities_include_streaming() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn mock_capabilities_include_tool_read() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolRead));
}

#[test]
fn capability_matrix_register_and_query() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    assert!(matrix.supports("mock", &Capability::Streaming));
    assert!(!matrix.supports("mock", &Capability::Vision));
}

#[test]
fn capability_matrix_backends_for() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    let backends = matrix.backends_for(&Capability::Streaming);
    assert_eq!(backends.len(), 2);
}

#[test]
fn capability_report_score() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register(
        "half",
        vec![Capability::Streaming], // 1 of 2
    );
    let report = matrix.evaluate("half", &[Capability::Streaming, Capability::Vision]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.missing.len(), 1);
}

// ===========================================================================
// 11. Multiple backends: register and use multiple backends
// ===========================================================================

#[test]
fn registry_multiple_backends() {
    let mut reg = abp_backend_core::BackendRegistry::new();
    for name in &["alpha", "beta", "gamma"] {
        reg.register_with_metadata(
            name,
            abp_backend_core::BackendMetadata {
                name: name.to_string(),
                dialect: "test".into(),
                version: "1".into(),
                max_tokens: None,
                supports_streaming: false,
                supports_tools: false,
                rate_limit: None,
            },
        );
    }
    assert_eq!(reg.len(), 3);
    let list = reg.list();
    assert!(list.contains(&"alpha"));
    assert!(list.contains(&"beta"));
    assert!(list.contains(&"gamma"));
}

#[tokio::test]
async fn multiple_backends_as_trait_objects() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(MockBackend),
        Box::new(StubBackend::new("a")),
        Box::new(StubBackend::new("b")),
    ];
    assert_eq!(backends[0].identity().id, "mock");
    assert_eq!(backends[1].identity().id, "a");
    assert_eq!(backends[2].identity().id, "b");
}

#[test]
fn selector_with_multiple_candidates() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![Capability::Streaming],
        priority: 1,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "b".into(),
        capabilities: vec![Capability::Streaming, Capability::ToolRead],
        priority: 2,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let selected = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(selected.name, "a");
}

#[test]
fn registry_by_dialect_filters_correctly() {
    let mut reg = abp_backend_core::BackendRegistry::new();
    reg.register_with_metadata(
        "openai1",
        abp_backend_core::BackendMetadata {
            name: "openai1".into(),
            dialect: "openai".into(),
            version: "1".into(),
            max_tokens: None,
            supports_streaming: true,
            supports_tools: true,
            rate_limit: None,
        },
    );
    reg.register_with_metadata(
        "claude1",
        abp_backend_core::BackendMetadata {
            name: "claude1".into(),
            dialect: "anthropic".into(),
            version: "1".into(),
            max_tokens: None,
            supports_streaming: true,
            supports_tools: true,
            rate_limit: None,
        },
    );
    let openai_backends = reg.by_dialect("openai");
    assert_eq!(openai_backends.len(), 1);
    assert!(openai_backends.contains(&"openai1"));
}

// ===========================================================================
// 12. Serde for backend config types
// ===========================================================================

#[test]
fn sidecar_spec_serde_roundtrip() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["index.js".into()],
        env: BTreeMap::from([("API_KEY".into(), "secret".into())]),
        cwd: Some("/app".into()),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.command, "node");
    assert_eq!(back.args, vec!["index.js"]);
    assert_eq!(back.cwd, Some("/app".into()));
}

#[test]
fn backend_metadata_serde_roundtrip() {
    let meta = abp_backend_core::BackendMetadata {
        name: "test".into(),
        dialect: "openai".into(),
        version: "2.0".into(),
        max_tokens: Some(8192),
        supports_streaming: true,
        supports_tools: false,
        rate_limit: Some(abp_backend_core::RateLimit {
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
            concurrent_requests: 5,
        }),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: abp_backend_core::BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "test");
    assert_eq!(back.max_tokens, Some(8192));
    assert!(back.supports_streaming);
}

#[test]
fn health_status_serde_roundtrip() {
    use abp_backend_core::HealthStatus;
    let statuses = [
        HealthStatus::Healthy,
        HealthStatus::Degraded,
        HealthStatus::Unhealthy,
        HealthStatus::Unknown,
    ];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let back: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status);
    }
}

#[test]
fn selection_strategy_serde_roundtrip() {
    let strategies = [
        SelectionStrategy::FirstMatch,
        SelectionStrategy::BestFit,
        SelectionStrategy::LeastLoaded,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::Priority,
    ];
    for strategy in &strategies {
        let json = serde_json::to_string(strategy).unwrap();
        let _back: SelectionStrategy = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn metrics_snapshot_serde_roundtrip() {
    let snap = MetricsSnapshot {
        total_runs: 10,
        successful_runs: 8,
        failed_runs: 2,
        total_events: 50,
        total_duration_ms: 5000,
        success_rate: 0.8,
        average_duration_ms: 500.0,
        average_events_per_run: 5.0,
    };
    let json = serde_json::to_string(&snap).unwrap();
    let back: MetricsSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_runs, 10);
    assert!((back.success_rate - 0.8).abs() < f64::EPSILON);
}

// ===========================================================================
// Bonus: health, metrics, additional edge cases
// ===========================================================================

#[test]
fn health_checker_empty_is_healthy() {
    let checker = HealthChecker::new();
    assert!(checker.is_healthy());
}

#[test]
fn health_checker_degraded_status() {
    let mut checker = HealthChecker::new();
    checker.add_check("net", HealthStatus::Healthy);
    checker.add_check(
        "disk",
        HealthStatus::Degraded {
            reason: "low space".into(),
        },
    );
    assert!(!checker.is_healthy());
    assert_eq!(checker.unhealthy_checks().len(), 1);
}

#[test]
fn metrics_record_and_query() {
    let m = BackendMetrics::new();
    m.record_run(true, 5, 100);
    m.record_run(false, 3, 200);
    assert_eq!(m.total_runs(), 2);
    assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 150.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_registry_get_or_create() {
    let reg = MetricsRegistry::new();
    let m1 = reg.get_or_create("backend_a");
    m1.record_run(true, 1, 10);
    let m2 = reg.get_or_create("backend_a");
    assert_eq!(m2.total_runs(), 1); // same Arc
}

#[test]
fn capability_matrix_common_capabilities() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register("b", vec![Capability::Streaming, Capability::Vision]);
    let common = matrix.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
}

#[test]
fn capability_matrix_empty_common() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.common_capabilities().is_empty());
}
