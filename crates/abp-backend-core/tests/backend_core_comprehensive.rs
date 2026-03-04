#![allow(clippy::all)]
#![allow(unknown_lints)]
//! Comprehensive tests for abp-backend-core: Backend trait, associated types,
//! mock implementations, trait object safety, serialization, and edge cases.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use abp_backend_core::health::{BackendHealth, HealthStatus};
use abp_backend_core::metadata::{BackendMetadata, RateLimit};
use abp_backend_core::registry::BackendRegistry;
use abp_backend_core::{
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
    Backend,
};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, ReceiptBuilder,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.into(),
        dialect: dialect.into(),
        version: "0.1.0".into(),
        max_tokens: None,
        supports_streaming: false,
        supports_tools: false,
        rate_limit: None,
    }
}

fn make_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn make_work_order_with_vendor(vendor: BTreeMap<String, serde_json::Value>) -> abp_core::WorkOrder {
    let config = RuntimeConfig {
        model: None,
        vendor,
        ..RuntimeConfig::default()
    };
    WorkOrderBuilder::new("task").config(config).build()
}

/// Minimal mock backend for trait tests.
struct MockBackend {
    id: String,
    caps: CapabilityManifest,
    call_count: Arc<AtomicU32>,
}

impl MockBackend {
    fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            caps: CapabilityManifest::new(),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn with_caps(id: &str, caps: CapabilityManifest) -> Self {
        Self {
            id: id.into(),
            caps,
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl Backend for MockBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.caps.clone()
    }

    async fn run(
        &self,
        run_id: Uuid,
        _work_order: abp_core::WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<abp_core::Receipt> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "started".into(),
                },
                ext: None,
            })
            .await;
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .await;
        Ok(ReceiptBuilder::new(&self.id)
            .work_order_id(run_id)
            .build())
    }
}

/// A backend that always fails.
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
        _work_order: abp_core::WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<abp_core::Receipt> {
        anyhow::bail!("intentional failure")
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Backend trait – identity & capabilities
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_returns_correct_id() {
    let b = MockBackend::new("test-backend");
    assert_eq!(b.identity().id, "test-backend");
}

#[test]
fn backend_identity_includes_version() {
    let b = MockBackend::new("v");
    assert_eq!(b.identity().backend_version, Some("1.0".into()));
}

#[test]
fn backend_identity_adapter_version_none() {
    let b = MockBackend::new("x");
    assert!(b.identity().adapter_version.is_none());
}

#[test]
fn backend_capabilities_empty_by_default() {
    let b = MockBackend::new("empty");
    assert!(b.capabilities().is_empty());
}

#[test]
fn backend_capabilities_returns_provided_caps() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let b = MockBackend::with_caps("streamer", caps);
    assert!(b.capabilities().contains_key(&Capability::Streaming));
}

#[test]
fn backend_capabilities_multiple_entries() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    caps.insert(Capability::Vision, SupportLevel::Unsupported);
    let b = MockBackend::with_caps("multi", caps);
    assert_eq!(b.capabilities().len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Backend trait – async run
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn backend_run_returns_receipt() {
    let b = MockBackend::new("ok");
    let (tx, _rx) = mpsc::channel(16);
    let receipt = b.run(Uuid::new_v4(), make_work_order(), tx).await.unwrap();
    assert_eq!(receipt.backend.id, "ok");
}

#[tokio::test]
async fn backend_run_sends_events() {
    let b = MockBackend::new("eventer");
    let (tx, mut rx) = mpsc::channel(16);
    let _ = b.run(Uuid::new_v4(), make_work_order(), tx).await.unwrap();
    let mut events = vec![];
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn backend_run_first_event_is_run_started() {
    let b = MockBackend::new("ev");
    let (tx, mut rx) = mpsc::channel(16);
    let _ = b.run(Uuid::new_v4(), make_work_order(), tx).await.unwrap();
    let first = rx.recv().await.unwrap();
    assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn backend_run_last_event_is_run_completed() {
    let b = MockBackend::new("ev");
    let (tx, mut rx) = mpsc::channel(16);
    let _ = b.run(Uuid::new_v4(), make_work_order(), tx).await.unwrap();
    let _first = rx.recv().await.unwrap();
    let second = rx.recv().await.unwrap();
    assert!(matches!(second.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn backend_run_propagates_run_id() {
    let run_id = Uuid::new_v4();
    let b = MockBackend::new("id-check");
    let (tx, _rx) = mpsc::channel(16);
    let receipt = b.run(run_id, make_work_order(), tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, run_id);
}

#[tokio::test]
async fn backend_run_increments_call_count() {
    let b = MockBackend::new("counter");
    let count = b.call_count.clone();
    let (tx, _rx) = mpsc::channel(16);
    let _ = b.run(Uuid::new_v4(), make_work_order(), tx).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn backend_run_multiple_times() {
    let b = MockBackend::new("multi");
    let count = b.call_count.clone();
    for _ in 0..3 {
        let (tx, _rx) = mpsc::channel(16);
        let _ = b.run(Uuid::new_v4(), make_work_order(), tx).await;
    }
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn failing_backend_returns_error() {
    let b = FailingBackend;
    let (tx, _rx) = mpsc::channel(16);
    let result = b.run(Uuid::new_v4(), make_work_order(), tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn failing_backend_error_message() {
    let b = FailingBackend;
    let (tx, _rx) = mpsc::channel(16);
    let err = b.run(Uuid::new_v4(), make_work_order(), tx).await.unwrap_err();
    assert!(err.to_string().contains("intentional failure"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Trait object safety
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_is_object_safe() {
    let b = MockBackend::new("obj");
    let _dyn_ref: &dyn Backend = &b;
}

#[test]
fn backend_trait_object_identity() {
    let b = MockBackend::new("dyn-id");
    let dyn_ref: &dyn Backend = &b;
    assert_eq!(dyn_ref.identity().id, "dyn-id");
}

#[test]
fn backend_trait_object_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let b = MockBackend::with_caps("dyn-caps", caps);
    let dyn_ref: &dyn Backend = &b;
    assert_eq!(dyn_ref.capabilities().len(), 1);
}

#[tokio::test]
async fn backend_trait_object_run() {
    let b = MockBackend::new("dyn-run");
    let dyn_ref: &dyn Backend = &b;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = dyn_ref
        .run(Uuid::new_v4(), make_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, "dyn-run");
}

#[test]
fn backend_boxed_trait_object() {
    let b: Box<dyn Backend> = Box::new(MockBackend::new("boxed"));
    assert_eq!(b.identity().id, "boxed");
}

#[tokio::test]
async fn backend_arc_trait_object() {
    let b: Arc<dyn Backend> = Arc::new(MockBackend::new("arc"));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = b
        .run(Uuid::new_v4(), make_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, "arc");
}

#[test]
fn backend_vec_of_trait_objects() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(MockBackend::new("a")),
        Box::new(MockBackend::new("b")),
        Box::new(FailingBackend),
    ];
    assert_eq!(backends.len(), 3);
    assert_eq!(backends[0].identity().id, "a");
    assert_eq!(backends[2].identity().id, "failing");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. ensure_capability_requirements
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_requirements_always_satisfied() {
    let reqs = CapabilityRequirements::default();
    let caps = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn native_requirement_satisfied_by_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn native_requirement_not_satisfied_by_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn emulated_requirement_satisfied_by_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn emulated_requirement_satisfied_by_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn emulated_requirement_satisfied_by_restricted() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(
        Capability::ToolUse,
        SupportLevel::Restricted {
            reason: "beta".into(),
        },
    );
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn requirement_not_satisfied_by_unsupported() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Vision, SupportLevel::Unsupported);
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn requirement_not_satisfied_when_capability_missing() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn multiple_requirements_all_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn multiple_requirements_one_unsatisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Vision,
                min_support: MinSupport::Native,
            },
        ],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    // Vision is missing
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    assert!(err.to_string().contains("unsatisfied"));
}

#[test]
fn unsatisfied_error_message_contains_capability_name() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let caps = CapabilityManifest::new();
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Vision"));
    assert!(msg.contains("missing"));
}

#[test]
fn unsatisfied_error_shows_actual_level() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Emulated"));
}

#[test]
fn native_requirement_not_satisfied_by_restricted() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Native,
        }],
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(
        Capability::ToolUse,
        SupportLevel::Restricted {
            reason: "test".into(),
        },
    );
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. extract_execution_mode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extract_mode_defaults_to_mapped() {
    let wo = make_work_order();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_nested_abp_object() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_nested_abp_mapped() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), serde_json::json!({"mode": "mapped"}));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_flat_key() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.mode".into(), serde_json::json!("passthrough"));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_nested_takes_precedence_over_flat() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        serde_json::json!({"mode": "passthrough"}),
    );
    vendor.insert("abp.mode".into(), serde_json::json!("mapped"));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_invalid_value_defaults_to_mapped() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), serde_json::json!({"mode": "invalid_mode"}));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_abp_key_not_object_defaults() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), serde_json::json!("not an object"));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_abp_object_missing_mode_defaults() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), serde_json::json!({"other_key": 42}));
    let wo = make_work_order_with_vendor(vendor);
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_empty_vendor_defaults() {
    let wo = make_work_order_with_vendor(BTreeMap::new());
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. validate_passthrough_compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validate_passthrough_always_ok() {
    let wo = make_work_order();
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

#[test]
fn validate_passthrough_with_custom_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("custom".into(), serde_json::json!({"flag": true}));
    let wo = make_work_order_with_vendor(vendor);
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. HealthStatus serialization & defaults
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn health_status_default_is_unknown() {
    assert_eq!(HealthStatus::default(), HealthStatus::Unknown);
}

#[test]
fn health_status_serialize_healthy() {
    let json = serde_json::to_string(&HealthStatus::Healthy).unwrap();
    assert_eq!(json, "\"healthy\"");
}

#[test]
fn health_status_serialize_degraded() {
    let json = serde_json::to_string(&HealthStatus::Degraded).unwrap();
    assert_eq!(json, "\"degraded\"");
}

#[test]
fn health_status_serialize_unhealthy() {
    let json = serde_json::to_string(&HealthStatus::Unhealthy).unwrap();
    assert_eq!(json, "\"unhealthy\"");
}

#[test]
fn health_status_serialize_unknown() {
    let json = serde_json::to_string(&HealthStatus::Unknown).unwrap();
    assert_eq!(json, "\"unknown\"");
}

#[test]
fn health_status_deserialize_all_variants() {
    for (s, expected) in [
        ("\"healthy\"", HealthStatus::Healthy),
        ("\"degraded\"", HealthStatus::Degraded),
        ("\"unhealthy\"", HealthStatus::Unhealthy),
        ("\"unknown\"", HealthStatus::Unknown),
    ] {
        let got: HealthStatus = serde_json::from_str(s).unwrap();
        assert_eq!(got, expected);
    }
}

#[test]
fn health_status_clone_eq() {
    let a = HealthStatus::Healthy;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn health_status_debug() {
    let s = format!("{:?}", HealthStatus::Degraded);
    assert!(s.contains("Degraded"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. BackendHealth defaults & serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn backend_health_default_values() {
    let h = BackendHealth::default();
    assert_eq!(h.status, HealthStatus::Unknown);
    assert!(h.last_check.is_none());
    assert!(h.latency_ms.is_none());
    assert!((h.error_rate - 0.0).abs() < f64::EPSILON);
    assert_eq!(h.consecutive_failures, 0);
}

#[test]
fn backend_health_serde_roundtrip_full() {
    let h = BackendHealth {
        status: HealthStatus::Healthy,
        last_check: Some(Utc::now()),
        latency_ms: Some(250),
        error_rate: 0.01,
        consecutive_failures: 0,
    };
    let json = serde_json::to_string(&h).unwrap();
    let h2: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(h2.status, HealthStatus::Healthy);
    assert_eq!(h2.latency_ms, Some(250));
}

#[test]
fn backend_health_serde_roundtrip_minimal() {
    let h = BackendHealth::default();
    let json = serde_json::to_string(&h).unwrap();
    let h2: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(h2.status, HealthStatus::Unknown);
    assert!(h2.last_check.is_none());
}

#[test]
fn backend_health_clone() {
    let h = BackendHealth {
        status: HealthStatus::Degraded,
        last_check: None,
        latency_ms: Some(100),
        error_rate: 0.5,
        consecutive_failures: 3,
    };
    let h2 = h.clone();
    assert_eq!(h2.status, h.status);
    assert_eq!(h2.consecutive_failures, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. RateLimit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rate_limit_serde_roundtrip() {
    let rl = RateLimit {
        requests_per_minute: 100,
        tokens_per_minute: 500_000,
        concurrent_requests: 10,
    };
    let json = serde_json::to_string(&rl).unwrap();
    let rl2: RateLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(rl, rl2);
}

#[test]
fn rate_limit_equality() {
    let a = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn rate_limit_inequality() {
    let a = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    let b = RateLimit {
        requests_per_minute: 120,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    assert_ne!(a, b);
}

#[test]
fn rate_limit_debug() {
    let rl = RateLimit {
        requests_per_minute: 1,
        tokens_per_minute: 2,
        concurrent_requests: 3,
    };
    let s = format!("{:?}", rl);
    assert!(s.contains("requests_per_minute"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. BackendMetadata serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn metadata_serde_roundtrip_no_rate_limit() {
    let m = make_metadata("test", "openai");
    let json = serde_json::to_string(&m).unwrap();
    let m2: BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.name, "test");
    assert_eq!(m2.dialect, "openai");
    assert!(m2.rate_limit.is_none());
}

#[test]
fn metadata_serde_roundtrip_with_rate_limit() {
    let m = BackendMetadata {
        name: "gpt4".into(),
        dialect: "openai".into(),
        version: "2.0".into(),
        max_tokens: Some(128_000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: Some(RateLimit {
            requests_per_minute: 500,
            tokens_per_minute: 1_000_000,
            concurrent_requests: 20,
        }),
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.rate_limit.unwrap().requests_per_minute, 500);
}

#[test]
fn metadata_clone() {
    let m = make_metadata("clone-test", "anthropic");
    let m2 = m.clone();
    assert_eq!(m2.name, "clone-test");
}

#[test]
fn metadata_debug() {
    let m = make_metadata("dbg", "openai");
    let s = format!("{:?}", m);
    assert!(s.contains("dbg"));
}

#[test]
fn metadata_max_tokens_none() {
    let m = make_metadata("no-tokens", "openai");
    assert!(m.max_tokens.is_none());
}

#[test]
fn metadata_supports_streaming_false() {
    let m = make_metadata("no-stream", "openai");
    assert!(!m.supports_streaming);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. BackendRegistry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let r = BackendRegistry::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn registry_default_is_empty() {
    let r = BackendRegistry::default();
    assert!(r.is_empty());
}

#[test]
fn registry_register_and_list() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("b", make_metadata("b", "openai"));
    r.register_with_metadata("a", make_metadata("a", "anthropic"));
    assert_eq!(r.list(), vec!["a", "b"]);
}

#[test]
fn registry_contains_after_register() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("x", make_metadata("x", "openai"));
    assert!(r.contains("x"));
    assert!(!r.contains("y"));
}

#[test]
fn registry_remove_returns_metadata() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("rm", make_metadata("rm", "openai"));
    let removed = r.remove("rm").unwrap();
    assert_eq!(removed.name, "rm");
    assert!(!r.contains("rm"));
    assert!(r.health("rm").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut r = BackendRegistry::new();
    assert!(r.remove("nope").is_none());
}

#[test]
fn registry_healthy_backends_sorted() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("z", make_metadata("z", "openai"));
    r.register_with_metadata("a", make_metadata("a", "openai"));
    r.update_health(
        "z",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    r.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    assert_eq!(r.healthy_backends(), vec!["a", "z"]);
}

#[test]
fn registry_by_dialect_sorted() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("gpt4", make_metadata("gpt4", "openai"));
    r.register_with_metadata("gpt3", make_metadata("gpt3", "openai"));
    r.register_with_metadata("claude", make_metadata("claude", "anthropic"));
    assert_eq!(r.by_dialect("openai"), vec!["gpt3", "gpt4"]);
    assert_eq!(r.by_dialect("anthropic"), vec!["claude"]);
    assert!(r.by_dialect("cohere").is_empty());
}

#[test]
fn registry_register_creates_default_health() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("new", make_metadata("new", "openai"));
    let h = r.health("new").unwrap();
    assert_eq!(h.status, HealthStatus::Unknown);
}

#[test]
fn registry_update_health_unregistered() {
    let mut r = BackendRegistry::new();
    r.update_health(
        "ghost",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    assert!(r.health("ghost").is_some());
    assert!(!r.contains("ghost"));
}

#[test]
fn registry_reregister_preserves_health() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("x", make_metadata("x", "openai"));
    r.update_health(
        "x",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    r.register_with_metadata("x", make_metadata("x-v2", "openai"));
    assert_eq!(r.health("x").unwrap().status, HealthStatus::Healthy);
    assert_eq!(r.metadata("x").unwrap().name, "x-v2");
}

#[test]
fn registry_len_tracks_metadata() {
    let mut r = BackendRegistry::new();
    assert_eq!(r.len(), 0);
    r.register_with_metadata("a", make_metadata("a", "openai"));
    assert_eq!(r.len(), 1);
    r.register_with_metadata("b", make_metadata("b", "openai"));
    assert_eq!(r.len(), 2);
    r.remove("a");
    assert_eq!(r.len(), 1);
}

#[test]
fn registry_is_empty_after_remove_all() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("a", make_metadata("a", "openai"));
    r.remove("a");
    assert!(r.is_empty());
}

#[test]
fn registry_clone() {
    let mut r = BackendRegistry::new();
    r.register_with_metadata("a", make_metadata("a", "openai"));
    let r2 = r.clone();
    assert_eq!(r2.len(), 1);
    assert!(r2.contains("a"));
}

#[test]
fn registry_debug() {
    let r = BackendRegistry::new();
    let s = format!("{:?}", r);
    assert!(s.contains("BackendRegistry"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Send + Sync bounds on Backend
// ═══════════════════════════════════════════════════════════════════════════

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn mock_backend_is_send() {
    assert_send::<MockBackend>();
}

#[test]
fn mock_backend_is_sync() {
    assert_sync::<MockBackend>();
}

#[test]
fn failing_backend_is_send_sync() {
    assert_send_sync::<FailingBackend>();
}

#[test]
fn boxed_backend_is_send() {
    assert_send::<Box<dyn Backend>>();
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Re-exports from lib.rs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reexport_backend_health() {
    let _: abp_backend_core::BackendHealth = BackendHealth::default();
}

#[test]
fn reexport_health_status() {
    let _: abp_backend_core::HealthStatus = HealthStatus::default();
}

#[test]
fn reexport_backend_metadata() {
    let _: abp_backend_core::BackendMetadata = make_metadata("re", "openai");
}

#[test]
fn reexport_rate_limit() {
    let _: abp_backend_core::RateLimit = RateLimit {
        requests_per_minute: 1,
        tokens_per_minute: 1,
        concurrent_requests: 1,
    };
}

#[test]
fn reexport_backend_registry() {
    let _: abp_backend_core::BackendRegistry = BackendRegistry::new();
}
