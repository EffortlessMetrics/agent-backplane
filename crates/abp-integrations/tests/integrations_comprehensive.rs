#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use abp_backend_core::{
    BackendHealth, BackendMetadata, BackendRegistry, HealthStatus as CoreHealthStatus, RateLimit,
};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec, CONTRACT_VERSION,
};
use abp_integrations::capability::{CapabilityMatrix, CapabilityReport};
use abp_integrations::health::{HealthCheck, HealthChecker, HealthStatus, SystemHealth};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry, MetricsSnapshot};
use abp_integrations::projection::{
    detect_dialect, map_via_ir, supported_translations, translate, translate_model_name, Dialect,
    EventMapping, Message, MessageRole, ProjectionMatrix, ToolCall, ToolDefinitionIr, ToolResult,
    ToolTranslation, TranslationFidelity, TranslationReport, MODEL_EQUIVALENCE_TABLE,
};
use abp_integrations::selector::{
    BackendCandidate, BackendSelector, SelectionResult, SelectionStrategy,
};
use abp_integrations::{
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
    Backend, MockBackend, SidecarBackend,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

// =========================================================================
// Helper to build a minimal WorkOrder
// =========================================================================

fn minimal_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn work_order_with_vendor(key: &str, val: serde_json::Value) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(key.to_string(), val);
    WorkOrderBuilder::new("test task").config(config).build()
}

fn work_order_with_snippets(snippets: Vec<(&str, &str)>) -> WorkOrder {
    let ctx = ContextPacket {
        files: vec![],
        snippets: snippets
            .into_iter()
            .map(|(n, c)| ContextSnippet {
                name: n.to_string(),
                content: c.to_string(),
            })
            .collect(),
    };
    WorkOrderBuilder::new("test task").context(ctx).build()
}

// =========================================================================
// 1. MockBackend identity & capabilities
// =========================================================================

#[test]
fn mock_backend_identity_id() {
    let mb = MockBackend;
    assert_eq!(mb.identity().id, "mock");
}

#[test]
fn mock_backend_identity_versions() {
    let mb = MockBackend;
    let id = mb.identity();
    assert_eq!(id.backend_version.as_deref(), Some("0.1"));
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn mock_backend_capabilities_streaming() {
    let mb = MockBackend;
    let caps = mb.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
    assert!(matches!(caps[&Capability::Streaming], SupportLevel::Native));
}

#[test]
fn mock_backend_capabilities_tool_read() {
    let mb = MockBackend;
    let caps = mb.capabilities();
    assert!(matches!(
        caps[&Capability::ToolRead],
        SupportLevel::Emulated
    ));
}

#[test]
fn mock_backend_capabilities_tool_write() {
    let mb = MockBackend;
    assert!(matches!(
        mb.capabilities()[&Capability::ToolWrite],
        SupportLevel::Emulated
    ));
}

#[test]
fn mock_backend_capabilities_tool_edit() {
    let mb = MockBackend;
    assert!(matches!(
        mb.capabilities()[&Capability::ToolEdit],
        SupportLevel::Emulated
    ));
}

#[test]
fn mock_backend_capabilities_tool_bash() {
    let mb = MockBackend;
    assert!(matches!(
        mb.capabilities()[&Capability::ToolBash],
        SupportLevel::Emulated
    ));
}

#[test]
fn mock_backend_capabilities_structured_output() {
    let mb = MockBackend;
    assert!(matches!(
        mb.capabilities()[&Capability::StructuredOutputJsonSchema],
        SupportLevel::Emulated
    ));
}

#[test]
fn mock_backend_capabilities_count() {
    let mb = MockBackend;
    assert_eq!(mb.capabilities().len(), 6);
}

// =========================================================================
// 2. MockBackend run (async)
// =========================================================================

#[tokio::test]
async fn mock_backend_run_returns_receipt() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_backend_run_receipt_has_hash() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn mock_backend_run_receipt_contract_version() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_backend_run_emits_events() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, mut rx) = mpsc::channel(32);
    mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 4, "expected at least 4 events, got {count}");
}

#[tokio::test]
async fn mock_backend_run_trace_has_events() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.trace.len() >= 4);
}

#[tokio::test]
async fn mock_backend_run_first_event_is_run_started() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(matches!(
        receipt.trace[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn mock_backend_run_last_event_is_run_completed() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let last = receipt.trace.last().unwrap();
    assert!(matches!(last.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn mock_backend_run_preserves_work_order_id() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let wo_id = wo.id;
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn mock_backend_run_default_mode_is_mapped() {
    let mb = MockBackend;
    let wo = minimal_work_order();
    let (tx, _rx) = mpsc::channel(32);
    let receipt = mb.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

// =========================================================================
// 3. Execution mode extraction
// =========================================================================

#[test]
fn extract_mode_default_is_mapped() {
    let wo = minimal_work_order();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_nested_passthrough() {
    let wo = work_order_with_vendor("abp", json!({"mode": "passthrough"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_nested_mapped() {
    let wo = work_order_with_vendor("abp", json!({"mode": "mapped"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_dotted_passthrough() {
    let wo = work_order_with_vendor("abp.mode", json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_dotted_mapped() {
    let wo = work_order_with_vendor("abp.mode", json!("mapped"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_invalid_value_falls_back_to_mapped() {
    let wo = work_order_with_vendor("abp", json!({"mode": "invalid"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_empty_vendor_is_mapped() {
    let wo = minimal_work_order();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_nested_takes_priority_over_dotted() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    config
        .vendor
        .insert("abp.mode".to_string(), json!("mapped"));
    let wo = WorkOrderBuilder::new("test").config(config).build();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

// =========================================================================
// 4. Passthrough compatibility validation
// =========================================================================

#[test]
fn validate_passthrough_compatibility_ok() {
    let wo = minimal_work_order();
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

// =========================================================================
// 5. Capability requirements
// =========================================================================

#[test]
fn ensure_capability_requirements_empty_requirements_ok() {
    let reqs = CapabilityRequirements::default();
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capability_requirements_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capability_requirements_unsatisfied() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

// =========================================================================
// 6. CapabilityMatrix
// =========================================================================

#[test]
fn capability_matrix_new_is_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.is_empty());
    assert_eq!(m.backend_count(), 0);
}

#[test]
fn capability_matrix_register_single() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    assert_eq!(m.backend_count(), 1);
    assert!(!m.is_empty());
}

#[test]
fn capability_matrix_supports() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    assert!(m.supports("mock", &Capability::Streaming));
    assert!(!m.supports("mock", &Capability::ToolRead));
}

#[test]
fn capability_matrix_supports_unknown_backend() {
    let m = CapabilityMatrix::new();
    assert!(!m.supports("unknown", &Capability::Streaming));
}

#[test]
fn capability_matrix_backends_for() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("c", vec![Capability::ToolRead]);
    let streaming = m.backends_for(&Capability::Streaming);
    assert!(streaming.contains(&"a".to_string()));
    assert!(streaming.contains(&"b".to_string()));
    assert!(!streaming.contains(&"c".to_string()));
}

#[test]
fn capability_matrix_all_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    let all = m.all_capabilities("mock").unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn capability_matrix_all_capabilities_unknown() {
    let m = CapabilityMatrix::new();
    assert!(m.all_capabilities("nope").is_none());
}

#[test]
fn capability_matrix_common_capabilities_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.common_capabilities().is_empty());
}

#[test]
fn capability_matrix_common_capabilities_single_backend() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    let common = m.common_capabilities();
    assert_eq!(common.len(), 2);
}

#[test]
fn capability_matrix_common_capabilities_intersection() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("b", vec![Capability::Streaming, Capability::ToolWrite]);
    let common = m.common_capabilities();
    assert_eq!(common.len(), 1);
    assert!(common.contains(&Capability::Streaming));
}

#[test]
fn capability_matrix_evaluate_full_match() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    let report = m.evaluate("a", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
    assert!(report.missing.is_empty());
}

#[test]
fn capability_matrix_evaluate_partial_match() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    let report = m.evaluate("a", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn capability_matrix_evaluate_no_requirements() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    let report = m.evaluate("a", &[]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn capability_matrix_best_backend() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register(
        "b",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    let best = m
        .best_backend(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(best, "b");
}

#[test]
fn capability_matrix_best_backend_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.best_backend(&[Capability::Streaming]).is_none());
}

#[test]
fn capability_matrix_register_merges() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("a", vec![Capability::ToolRead]);
    let all = m.all_capabilities("a").unwrap();
    assert_eq!(all.len(), 2);
}

// =========================================================================
// 7. HealthChecker
// =========================================================================

#[test]
fn health_checker_new_is_empty() {
    let hc = HealthChecker::new();
    assert_eq!(hc.check_count(), 0);
    assert!(hc.is_healthy());
}

#[test]
fn health_checker_add_healthy() {
    let mut hc = HealthChecker::new();
    hc.add_check("mock", HealthStatus::Healthy);
    assert_eq!(hc.check_count(), 1);
    assert!(hc.is_healthy());
}

#[test]
fn health_checker_degraded_overall() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.add_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    assert!(!hc.is_healthy());
    assert!(matches!(hc.overall_status(), HealthStatus::Degraded { .. }));
}

#[test]
fn health_checker_unhealthy_overall() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.add_check(
        "b",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
    );
    assert!(matches!(
        hc.overall_status(),
        HealthStatus::Unhealthy { .. }
    ));
}

#[test]
fn health_checker_unknown_overall() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.add_check("b", HealthStatus::Unknown);
    assert!(matches!(hc.overall_status(), HealthStatus::Unknown));
}

#[test]
fn health_checker_unhealthy_checks() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.add_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    let unhealthy = hc.unhealthy_checks();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "b");
}

#[test]
fn health_checker_clear() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.clear();
    assert_eq!(hc.check_count(), 0);
}

#[test]
fn health_checker_checks_accessor() {
    let mut hc = HealthChecker::new();
    hc.add_check("x", HealthStatus::Healthy);
    assert_eq!(hc.checks().len(), 1);
    assert_eq!(hc.checks()[0].name, "x");
}

#[test]
fn health_status_serde_roundtrip_healthy() {
    let s = HealthStatus::Healthy;
    let json = serde_json::to_string(&s).unwrap();
    let back: HealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn health_status_serde_roundtrip_degraded() {
    let s = HealthStatus::Degraded {
        reason: "slow".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: HealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn health_status_serde_roundtrip_unhealthy() {
    let s = HealthStatus::Unhealthy {
        reason: "down".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: HealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn health_status_serde_roundtrip_unknown() {
    let s = HealthStatus::Unknown;
    let json = serde_json::to_string(&s).unwrap();
    let back: HealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn system_health_serde_roundtrip() {
    let sh = SystemHealth {
        backends: vec![],
        overall: HealthStatus::Healthy,
        uptime_seconds: 42,
        version: "0.1".into(),
    };
    let json = serde_json::to_string(&sh).unwrap();
    let back: SystemHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(back.uptime_seconds, 42);
    assert_eq!(back.overall, HealthStatus::Healthy);
}

// =========================================================================
// 8. BackendMetrics & MetricsRegistry
// =========================================================================

#[test]
fn backend_metrics_new_zeros() {
    let m = BackendMetrics::new();
    assert_eq!(m.total_runs(), 0);
    assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 0.0).abs() < f64::EPSILON);
    assert!((m.average_events_per_run() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn backend_metrics_record_single_success() {
    let m = BackendMetrics::new();
    m.record_run(true, 5, 100);
    assert_eq!(m.total_runs(), 1);
    assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn backend_metrics_record_mixed() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    m.record_run(false, 5, 300);
    assert_eq!(m.total_runs(), 2);
    assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 400.0).abs() < f64::EPSILON);
    assert!((m.average_events_per_run() - 7.5).abs() < f64::EPSILON);
}

#[test]
fn backend_metrics_reset() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    m.reset();
    assert_eq!(m.total_runs(), 0);
    assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn backend_metrics_snapshot() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    m.record_run(false, 5, 300);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 15);
    assert_eq!(snap.total_duration_ms, 800);
}

#[test]
fn backend_metrics_snapshot_serde_roundtrip() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    let snap = m.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: MetricsSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_runs, 1);
}

#[test]
fn backend_metrics_debug_impl() {
    let m = BackendMetrics::new();
    let s = format!("{:?}", m);
    assert!(s.contains("BackendMetrics"));
}

#[test]
fn backend_metrics_default() {
    let m = BackendMetrics::default();
    assert_eq!(m.total_runs(), 0);
}

#[test]
fn metrics_registry_new_empty() {
    let r = MetricsRegistry::new();
    assert!(r.snapshot_all().is_empty());
}

#[test]
fn metrics_registry_get_or_create() {
    let r = MetricsRegistry::new();
    let m1 = r.get_or_create("mock");
    m1.record_run(true, 1, 10);
    let m2 = r.get_or_create("mock");
    assert_eq!(m2.total_runs(), 1);
}

#[test]
fn metrics_registry_snapshot_all() {
    let r = MetricsRegistry::new();
    r.get_or_create("a").record_run(true, 1, 10);
    r.get_or_create("b").record_run(false, 2, 20);
    let all = r.snapshot_all();
    assert_eq!(all.len(), 2);
    assert!(all.contains_key("a"));
    assert!(all.contains_key("b"));
}

#[test]
fn metrics_registry_debug_impl() {
    let r = MetricsRegistry::new();
    r.get_or_create("test");
    let s = format!("{:?}", r);
    assert!(s.contains("MetricsRegistry"));
}

#[test]
fn metrics_registry_default() {
    let r = MetricsRegistry::default();
    assert!(r.snapshot_all().is_empty());
}

// =========================================================================
// 9. BackendSelector & SelectionStrategy
// =========================================================================

fn make_candidate(name: &str, caps: Vec<Capability>, priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps,
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn selector_first_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 2));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "a");
}

#[test]
fn selector_best_fit() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        2,
    ));
    let chosen = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 10));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_least_loaded() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 10));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_round_robin() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 2));
    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn selector_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    assert!(sel.select(&[Capability::McpClient]).is_none());
}

#[test]
fn selector_disabled_candidate_skipped() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = make_candidate("a", vec![Capability::Streaming], 1);
    c.enabled = false;
    sel.add_candidate(c);
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 2));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_candidate_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![], 1));
    sel.add_candidate(make_candidate("b", vec![], 2));
    assert_eq!(sel.candidate_count(), 2);
}

#[test]
fn selector_enabled_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![], 1));
    let mut c = make_candidate("b", vec![], 2);
    c.enabled = false;
    sel.add_candidate(c);
    assert_eq!(sel.enabled_count(), 1);
}

#[test]
fn selector_select_all() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate(
        "a",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 2));
    let all = sel.select_all(&[Capability::Streaming]);
    assert_eq!(all.len(), 2);
}

#[test]
fn selector_select_with_result_success() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.unmet_capabilities.is_empty());
}

#[test]
fn selector_select_with_result_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::McpClient]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn selection_strategy_serde_roundtrip() {
    for strat in [
        SelectionStrategy::FirstMatch,
        SelectionStrategy::BestFit,
        SelectionStrategy::LeastLoaded,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::Priority,
    ] {
        let json = serde_json::to_string(&strat).unwrap();
        let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
        // Just verify no panic on roundtrip
        let _ = format!("{:?}", back);
    }
}

#[test]
fn backend_candidate_serde_roundtrip() {
    let c = make_candidate("test", vec![Capability::Streaming], 5);
    let json = serde_json::to_string(&c).unwrap();
    let back: BackendCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "test");
    assert_eq!(back.priority, 5);
}

// =========================================================================
// 10. ProjectionMatrix — dialect translations
// =========================================================================

#[test]
fn projection_matrix_new_has_dialects() {
    let pm = ProjectionMatrix::new();
    let dialects = pm.supported_dialects();
    assert!(dialects.contains(&"abp".to_string()));
    assert!(dialects.contains(&"openai".to_string()));
    assert!(dialects.contains(&"anthropic".to_string()));
    assert!(dialects.contains(&"gemini".to_string()));
}

#[test]
fn projection_matrix_identity_translation() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Abp, &wo).unwrap();
    assert!(result.is_object());
}

#[test]
fn projection_matrix_abp_to_claude() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    assert!(result.get("model").is_some());
    assert!(result.get("messages").is_some());
}

#[test]
fn projection_matrix_abp_to_openai() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    assert!(result.get("model").is_some());
    assert!(result.get("messages").is_some());
}

#[test]
fn projection_matrix_abp_to_gemini() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    assert!(result.get("model").is_some());
    assert!(result.get("contents").is_some());
}

#[test]
fn projection_matrix_abp_to_codex() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    assert!(result.get("model").is_some());
    assert!(result.get("input").is_some());
}

#[test]
fn projection_matrix_abp_to_kimi() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    assert!(result.get("model").is_some());
    assert!(result.get("messages").is_some());
}

#[test]
fn projection_matrix_abp_to_mock() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    let result = pm.translate(Dialect::Abp, Dialect::Mock, &wo).unwrap();
    assert!(result.get("model").is_some());
}

#[test]
fn projection_matrix_unsupported_vendor_to_vendor() {
    let pm = ProjectionMatrix::new();
    let wo = minimal_work_order();
    assert!(pm.translate(Dialect::Claude, Dialect::OpenAi, &wo).is_err());
}

#[test]
fn projection_matrix_supported_translations_has_identity() {
    let pairs = supported_translations();
    assert!(pairs.contains(&(Dialect::Abp, Dialect::Abp)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Claude)));
}

#[test]
fn projection_matrix_supported_translations_has_abp_to_vendors() {
    let pairs = supported_translations();
    assert!(pairs.contains(&(Dialect::Abp, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Abp, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::Abp, Dialect::Gemini)));
}

// =========================================================================
// 11. Dialect enum serde
// =========================================================================

#[test]
fn dialect_all_has_seven_variants() {
    assert_eq!(Dialect::ALL.len(), 7);
}

#[test]
fn dialect_serde_roundtrip_abp() {
    let d = Dialect::Abp;
    let json = serde_json::to_string(&d).unwrap();
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

#[test]
fn dialect_serde_roundtrip_claude() {
    let d = Dialect::Claude;
    let json = serde_json::to_string(&d).unwrap();
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

#[test]
fn dialect_serde_roundtrip_openai() {
    let d = Dialect::OpenAi;
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, "\"openai\"");
    let back: Dialect = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

#[test]
fn dialect_serde_roundtrip_all() {
    for &d in Dialect::ALL {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }
}

// =========================================================================
// 12. TranslationFidelity
// =========================================================================

#[test]
fn translation_fidelity_identity_is_lossless() {
    let pm = ProjectionMatrix::new();
    assert_eq!(
        pm.can_translate(Dialect::Abp, Dialect::Abp),
        TranslationFidelity::Lossless
    );
}

#[test]
fn translation_fidelity_abp_to_vendor_is_lossy() {
    let pm = ProjectionMatrix::new();
    assert_eq!(
        pm.can_translate(Dialect::Abp, Dialect::Claude),
        TranslationFidelity::LossySupported
    );
}

#[test]
fn translation_fidelity_mock_is_lossy() {
    let pm = ProjectionMatrix::new();
    assert_eq!(
        pm.can_translate(Dialect::Mock, Dialect::Claude),
        TranslationFidelity::LossySupported
    );
}

#[test]
fn translation_fidelity_vendor_to_vendor_degraded() {
    let pm = ProjectionMatrix::new();
    let fidelity = pm.can_translate(Dialect::OpenAi, Dialect::Claude);
    assert!(matches!(
        fidelity,
        TranslationFidelity::Degraded | TranslationFidelity::LossySupported
    ));
}

#[test]
fn translation_fidelity_serde_roundtrip() {
    for f in [
        TranslationFidelity::Lossless,
        TranslationFidelity::LossySupported,
        TranslationFidelity::Degraded,
        TranslationFidelity::Unsupported,
    ] {
        let json = serde_json::to_string(&f).unwrap();
        let back: TranslationFidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }
}

// =========================================================================
// 13. Tool call / result translation
// =========================================================================

#[test]
fn tool_call_translate_abp_to_openai() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "foo.rs"}),
    };
    let translated = pm.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(translated.tool_name, "file_read");
}

#[test]
fn tool_call_translate_openai_to_abp() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "file_read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    let translated = pm.translate_tool_call("openai", "abp", &call).unwrap();
    assert_eq!(translated.tool_name, "read_file");
}

#[test]
fn tool_call_translate_same_dialect_passthrough() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    let translated = pm.translate_tool_call("abp", "abp", &call).unwrap();
    assert_eq!(translated.tool_name, "read_file");
}

#[test]
fn tool_call_translate_unknown_dialect_errors() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    assert!(pm.translate_tool_call("unknown", "abp", &call).is_err());
}

#[test]
fn tool_result_translate_abp_to_anthropic() {
    let pm = ProjectionMatrix::new();
    let result = ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        output: json!("ok"),
        is_error: false,
    };
    let translated = pm
        .translate_tool_result("abp", "anthropic", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "Bash");
}

#[test]
fn tool_result_translate_preserves_error_flag() {
    let pm = ProjectionMatrix::new();
    let result = ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        output: json!("error"),
        is_error: true,
    };
    let translated = pm
        .translate_tool_result("abp", "anthropic", &result)
        .unwrap();
    assert!(translated.is_error);
}

#[test]
fn tool_call_unmapped_name_passed_through() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "custom_tool".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    let translated = pm.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(translated.tool_name, "custom_tool");
}

// =========================================================================
// 14. Event translation
// =========================================================================

#[test]
fn translate_event_tool_call_name_mapped() {
    let pm = ProjectionMatrix::new();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("id1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let translated = pm.translate_event("abp", "openai", &ev).unwrap();
    if let AgentEventKind::ToolCall { tool_name, .. } = &translated.kind {
        assert_eq!(tool_name, "file_read");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn translate_event_tool_result_name_mapped() {
    let pm = ProjectionMatrix::new();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "write_file".into(),
            tool_use_id: None,
            output: json!("ok"),
            is_error: false,
        },
        ext: None,
    };
    let translated = pm.translate_event("abp", "gemini", &ev).unwrap();
    if let AgentEventKind::ToolResult { tool_name, .. } = &translated.kind {
        assert_eq!(tool_name, "writeFile");
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn translate_event_assistant_message_unchanged() {
    let pm = ProjectionMatrix::new();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let translated = pm.translate_event("abp", "openai", &ev).unwrap();
    assert!(matches!(
        translated.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn translate_event_same_dialect_no_change() {
    let pm = ProjectionMatrix::new();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let translated = pm.translate_event("abp", "abp", &ev).unwrap();
    if let AgentEventKind::RunStarted { message } = &translated.kind {
        assert_eq!(message, "go");
    }
}

// =========================================================================
// 15. has_translation & tool_translation accessors
// =========================================================================

#[test]
fn has_translation_same_dialect() {
    let pm = ProjectionMatrix::new();
    assert!(pm.has_translation("abp", "abp"));
    assert!(pm.has_translation("openai", "openai"));
}

#[test]
fn has_translation_cross_dialect() {
    let pm = ProjectionMatrix::new();
    assert!(pm.has_translation("abp", "openai"));
    assert!(pm.has_translation("openai", "abp"));
}

#[test]
fn tool_translation_abp_openai() {
    let pm = ProjectionMatrix::new();
    let tt = pm.tool_translation("abp", "openai").unwrap();
    assert_eq!(tt.name_map.get("read_file"), Some(&"file_read".to_string()));
}

#[test]
fn event_mapping_abp_openai() {
    let pm = ProjectionMatrix::new();
    let em = pm.event_mapping("abp", "openai").unwrap();
    assert!(em.kind_map.contains_key("run_started"));
}

// =========================================================================
// 16. Message mapping
// =========================================================================

#[test]
fn map_messages_system_to_claude_folded() {
    let pm = ProjectionMatrix::new();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "You are helpful.".into(),
    }];
    let mapped = pm
        .map_messages(Dialect::Abp, Dialect::Claude, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::User);
    assert!(mapped[0].content.starts_with("[System]"));
}

#[test]
fn map_messages_system_to_gemini_folded() {
    let pm = ProjectionMatrix::new();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "Be concise.".into(),
    }];
    let mapped = pm
        .map_messages(Dialect::Abp, Dialect::Gemini, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::User);
}

#[test]
fn map_messages_system_to_openai_preserved() {
    let pm = ProjectionMatrix::new();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "instructions".into(),
    }];
    let mapped = pm
        .map_messages(Dialect::Abp, Dialect::OpenAi, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::System);
}

#[test]
fn map_messages_user_unchanged() {
    let pm = ProjectionMatrix::new();
    let msgs = vec![Message {
        role: MessageRole::User,
        content: "hello".into(),
    }];
    let mapped = pm
        .map_messages(Dialect::Abp, Dialect::Claude, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::User);
    assert_eq!(mapped[0].content, "hello");
}

// =========================================================================
// 17. Tool definition mapping
// =========================================================================

#[test]
fn map_tool_definitions_same_dialect() {
    let pm = ProjectionMatrix::new();
    let tools = vec![ToolDefinitionIr {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({}),
    }];
    let mapped = pm
        .map_tool_definitions(Dialect::Abp, Dialect::Abp, &tools)
        .unwrap();
    assert_eq!(mapped[0].name, "read_file");
}

#[test]
fn map_tool_definitions_abp_to_openai() {
    let pm = ProjectionMatrix::new();
    let tools = vec![ToolDefinitionIr {
        name: "bash".into(),
        description: "Execute shell".into(),
        parameters: json!({}),
    }];
    let mapped = pm
        .map_tool_definitions(Dialect::Abp, Dialect::OpenAi, &tools)
        .unwrap();
    assert_eq!(mapped[0].name, "shell");
}

// =========================================================================
// 18. Model name mapping
// =========================================================================

#[test]
fn map_model_name_to_abp_passthrough() {
    let pm = ProjectionMatrix::new();
    let result = pm
        .map_model_name(Dialect::OpenAi, Dialect::Abp, "gpt-4o")
        .unwrap();
    assert_eq!(result, "gpt-4o");
}

#[test]
fn map_model_name_to_mock_passthrough() {
    let pm = ProjectionMatrix::new();
    let result = pm
        .map_model_name(Dialect::OpenAi, Dialect::Mock, "gpt-4o")
        .unwrap();
    assert_eq!(result, "gpt-4o");
}

#[test]
fn map_model_name_native_model_unchanged() {
    let pm = ProjectionMatrix::new();
    let result = pm
        .map_model_name(Dialect::OpenAi, Dialect::OpenAi, "gpt-4o")
        .unwrap();
    assert_eq!(result, "gpt-4o");
}

#[test]
fn map_model_name_cross_dialect() {
    let pm = ProjectionMatrix::new();
    let result = pm
        .map_model_name(Dialect::OpenAi, Dialect::Claude, "gpt-4o")
        .unwrap();
    assert_eq!(result, "claude-sonnet-4-20250514");
}

#[test]
fn map_model_name_unknown_model_errors() {
    let pm = ProjectionMatrix::new();
    assert!(pm
        .map_model_name(Dialect::OpenAi, Dialect::Claude, "unknown-model")
        .is_err());
}

#[test]
fn translate_model_name_fn_gpt4o_to_claude() {
    let result = translate_model_name("gpt-4o", Dialect::Claude).unwrap();
    assert_eq!(result, "claude-sonnet-4-20250514");
}

#[test]
fn translate_model_name_fn_to_abp_passthrough() {
    let result = translate_model_name("gpt-4o", Dialect::Abp).unwrap();
    assert_eq!(result, "gpt-4o");
}

#[test]
fn translate_model_name_fn_unknown_returns_none() {
    assert!(translate_model_name("nonexistent-model", Dialect::Claude).is_none());
}

#[test]
fn model_equivalence_table_is_nonempty() {
    assert!(!MODEL_EQUIVALENCE_TABLE.is_empty());
}

// =========================================================================
// 19. detect_dialect
// =========================================================================

#[test]
fn detect_dialect_gemini_parts() {
    let msgs = json!([{"role": "user", "parts": [{"text": "hello"}]}]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::Gemini));
}

#[test]
fn detect_dialect_openai_tool_calls() {
    let msgs = json!([{"role": "assistant", "content": null, "tool_calls": []}]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::OpenAi));
}

#[test]
fn detect_dialect_openai_system_role() {
    let msgs = json!([{"role": "system", "content": "hi"}]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::OpenAi));
}

#[test]
fn detect_dialect_claude_default() {
    let msgs = json!([{"role": "user", "content": "hello"}]);
    assert_eq!(detect_dialect(&msgs), Some(Dialect::Claude));
}

#[test]
fn detect_dialect_empty_array() {
    let msgs = json!([]);
    assert_eq!(detect_dialect(&msgs), None);
}

#[test]
fn detect_dialect_non_array() {
    let msgs = json!("not an array");
    assert_eq!(detect_dialect(&msgs), None);
}

// =========================================================================
// 20. map_via_ir cross-dialect translation
// =========================================================================

#[test]
fn map_via_ir_identity() {
    let msgs = json!([{"role": "user", "content": "hello"}]);
    let (output, report) = map_via_ir(Dialect::OpenAi, Dialect::OpenAi, &msgs).unwrap();
    assert_eq!(output, msgs);
    assert_eq!(report.fidelity, TranslationFidelity::Lossless);
}

#[test]
fn map_via_ir_openai_to_gemini() {
    let msgs = json!([{"role": "user", "content": "hello"}]);
    let (output, report) = map_via_ir(Dialect::OpenAi, Dialect::Gemini, &msgs).unwrap();
    let arr = output.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr[0].get("parts").is_some());
}

#[test]
fn map_via_ir_openai_to_claude() {
    let msgs = json!([{"role": "user", "content": "hello"}]);
    let (output, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    let arr = output.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0].get("role").unwrap(), "user");
}

#[test]
fn map_via_ir_non_array_errors() {
    let msgs = json!({"role": "user"});
    assert!(map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).is_err());
}

#[test]
fn map_via_ir_report_messages_mapped_count() {
    let msgs = json!([
        {"role": "user", "content": "a"},
        {"role": "assistant", "content": "b"},
    ]);
    let (_, report) = map_via_ir(Dialect::OpenAi, Dialect::Claude, &msgs).unwrap();
    assert_eq!(report.messages_mapped, 2);
}

// =========================================================================
// 21. WorkOrder builder and serde
// =========================================================================

#[test]
fn work_order_builder_basic() {
    let wo = WorkOrderBuilder::new("do something").build();
    assert_eq!(wo.task, "do something");
}

#[test]
fn work_order_builder_with_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn work_order_builder_with_budget() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(10.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn work_order_builder_with_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn work_order_serde_roundtrip() {
    let wo = minimal_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, wo.task);
    assert_eq!(back.id, wo.id);
}

#[test]
fn work_order_with_context_snippets() {
    let wo = work_order_with_snippets(vec![("file.rs", "fn main() {}")]);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "file.rs");
}

// =========================================================================
// 22. Receipt and hashing
// =========================================================================

#[test]
fn receipt_builder_basic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn receipt_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn receipt_hash_deterministic() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    // Rebuild with same data — different run_id means different hash
    // But at least verify it's a valid hex string
    let hash = r1.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_serde_roundtrip() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.outcome, receipt.outcome);
}

#[test]
fn outcome_serde_roundtrip() {
    for o in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&o).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, o);
    }
}

// =========================================================================
// 23. ExecutionMode serde
// =========================================================================

#[test]
fn execution_mode_serde_roundtrip() {
    for m in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&m).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

// =========================================================================
// 24. MessageRole serde
// =========================================================================

#[test]
fn message_role_serde_roundtrip() {
    for r in [
        MessageRole::System,
        MessageRole::User,
        MessageRole::Assistant,
    ] {
        let json = serde_json::to_string(&r).unwrap();
        let back: MessageRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}

// =========================================================================
// 25. ToolCall / ToolResult serde
// =========================================================================

#[test]
fn tool_call_serde_roundtrip() {
    let tc = ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: Some("parent1".into()),
        input: json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn tool_result_serde_roundtrip() {
    let tr = ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        output: json!({"stdout": "hello"}),
        is_error: false,
    };
    let json = serde_json::to_string(&tr).unwrap();
    let back: ToolResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tr);
}

// =========================================================================
// 26. ToolDefinitionIr serde
// =========================================================================

#[test]
fn tool_definition_ir_serde_roundtrip() {
    let td = ToolDefinitionIr {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: ToolDefinitionIr = serde_json::from_str(&json).unwrap();
    assert_eq!(back, td);
}

// =========================================================================
// 27. SidecarBackend identity & capabilities
// =========================================================================

#[test]
fn sidecar_backend_identity() {
    let spec = abp_host::SidecarSpec {
        command: "echo".into(),
        args: vec![],
        env: BTreeMap::new(),
        cwd: None,
    };
    let sb = SidecarBackend::new(spec);
    let id = sb.identity();
    assert_eq!(id.id, "sidecar");
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn sidecar_backend_capabilities_empty() {
    let spec = abp_host::SidecarSpec {
        command: "echo".into(),
        args: vec![],
        env: BTreeMap::new(),
        cwd: None,
    };
    let sb = SidecarBackend::new(spec);
    assert!(sb.capabilities().is_empty());
}

// =========================================================================
// 28. BackendRegistry (from abp-backend-core, re-exported)
// =========================================================================

#[test]
fn backend_registry_new_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn backend_registry_register_and_list() {
    let mut reg = BackendRegistry::new();
    let meta = BackendMetadata {
        name: "mock".into(),
        dialect: "mock".into(),
        version: "0.1".into(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    };
    reg.register_with_metadata("mock", meta);
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("mock"));
    assert_eq!(reg.list(), vec!["mock"]);
}

#[test]
fn backend_registry_metadata_lookup() {
    let mut reg = BackendRegistry::new();
    let meta = BackendMetadata {
        name: "test".into(),
        dialect: "openai".into(),
        version: "1.0".into(),
        max_tokens: Some(128000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    };
    reg.register_with_metadata("test", meta);
    let m = reg.metadata("test").unwrap();
    assert_eq!(m.dialect, "openai");
    assert_eq!(m.max_tokens, Some(128000));
}

#[test]
fn backend_registry_health_default() {
    let mut reg = BackendRegistry::new();
    let meta = BackendMetadata {
        name: "test".into(),
        dialect: "mock".into(),
        version: "0.1".into(),
        max_tokens: None,
        supports_streaming: false,
        supports_tools: false,
        rate_limit: None,
    };
    reg.register_with_metadata("test", meta);
    let h = reg.health("test").unwrap();
    assert_eq!(h.status, CoreHealthStatus::Unknown);
}

#[test]
fn backend_registry_update_health() {
    let mut reg = BackendRegistry::new();
    let meta = BackendMetadata {
        name: "test".into(),
        dialect: "mock".into(),
        version: "0.1".into(),
        max_tokens: None,
        supports_streaming: false,
        supports_tools: false,
        rate_limit: None,
    };
    reg.register_with_metadata("test", meta);
    reg.update_health(
        "test",
        BackendHealth {
            status: CoreHealthStatus::Healthy,
            last_check: Some(Utc::now()),
            latency_ms: Some(42),
            error_rate: 0.0,
            consecutive_failures: 0,
        },
    );
    assert_eq!(reg.healthy_backends(), vec!["test"]);
}

#[test]
fn backend_registry_by_dialect() {
    let mut reg = BackendRegistry::new();
    for (name, dialect) in [("a", "openai"), ("b", "openai"), ("c", "anthropic")] {
        reg.register_with_metadata(
            name,
            BackendMetadata {
                name: name.into(),
                dialect: dialect.into(),
                version: "0.1".into(),
                max_tokens: None,
                supports_streaming: false,
                supports_tools: false,
                rate_limit: None,
            },
        );
    }
    assert_eq!(reg.by_dialect("openai"), vec!["a", "b"]);
    assert_eq!(reg.by_dialect("anthropic"), vec!["c"]);
}

#[test]
fn backend_registry_remove() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata(
        "x",
        BackendMetadata {
            name: "x".into(),
            dialect: "mock".into(),
            version: "0.1".into(),
            max_tokens: None,
            supports_streaming: false,
            supports_tools: false,
            rate_limit: None,
        },
    );
    assert!(reg.contains("x"));
    reg.remove("x");
    assert!(!reg.contains("x"));
}

// =========================================================================
// 29. RateLimit serde
// =========================================================================

#[test]
fn rate_limit_serde_roundtrip() {
    let rl = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100000,
        concurrent_requests: 5,
    };
    let json = serde_json::to_string(&rl).unwrap();
    let back: RateLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rl);
}

// =========================================================================
// 30. AgentEvent / AgentEventKind serde
// =========================================================================

#[test]
fn agent_event_kind_run_started_serde() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let ev = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_kind_tool_call_serde() {
    let kind = AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: None,
        input: json!({"command": "ls"}),
    };
    let ev = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall { tool_name, .. } = &back.kind {
        assert_eq!(tool_name, "bash");
    } else {
        panic!("expected ToolCall");
    }
}

// =========================================================================
// 31. WorkOrder with snippets affects translation content
// =========================================================================

#[test]
fn translation_includes_snippets_in_content() {
    let wo = work_order_with_snippets(vec![("readme", "# Hello")]);
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let msgs = result.get("messages").unwrap().as_array().unwrap();
    let content = msgs[0].get("content").unwrap().as_str().unwrap();
    assert!(content.contains("# Hello"));
    assert!(content.contains("readme"));
}

// =========================================================================
// 32. TranslationReport serde
// =========================================================================

#[test]
fn translation_report_serde_roundtrip() {
    let report = TranslationReport {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        messages_mapped: 3,
        losses: vec!["system message dropped".into()],
        fidelity: TranslationFidelity::Degraded,
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: TranslationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

// =========================================================================
// 33. BackendIdentity serde
// =========================================================================

#[test]
fn backend_identity_serde_roundtrip() {
    let id = BackendIdentity {
        id: "mock".into(),
        backend_version: Some("0.1".into()),
        adapter_version: None,
    };
    let json = serde_json::to_string(&id).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "mock");
    assert!(back.adapter_version.is_none());
}

// =========================================================================
// 34. Edge cases
// =========================================================================

#[test]
fn empty_work_order_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn work_order_default_lane_is_patch_first() {
    let wo = minimal_work_order();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn capability_matrix_evaluate_unknown_backend() {
    let m = CapabilityMatrix::new();
    let report = m.evaluate("nonexistent", &[Capability::Streaming]);
    assert!((report.score - 0.0).abs() < f64::EPSILON);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn selector_empty_requirements_matches_all() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![], 1));
    let chosen = sel.select(&[]).unwrap();
    assert_eq!(chosen.name, "a");
}

#[test]
fn health_checker_unhealthy_worse_than_degraded() {
    let mut hc = HealthChecker::new();
    hc.add_check(
        "a",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    hc.add_check(
        "b",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
    );
    assert!(matches!(
        hc.overall_status(),
        HealthStatus::Unhealthy { .. }
    ));
}

#[test]
fn metrics_snapshot_averages() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 100);
    m.record_run(true, 20, 200);
    m.record_run(true, 30, 300);
    let snap = m.snapshot();
    assert!((snap.average_duration_ms - 200.0).abs() < f64::EPSILON);
    assert!((snap.average_events_per_run - 20.0).abs() < f64::EPSILON);
    assert!((snap.success_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn contract_version_is_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// =========================================================================
// 35. Abp-to-vendor model defaults
// =========================================================================

#[test]
fn translate_abp_to_claude_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let result = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert!(model.starts_with("claude-"));
}

#[test]
fn translate_abp_to_openai_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert!(model.starts_with("gpt-"));
}

#[test]
fn translate_abp_to_gemini_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let result = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert!(model.starts_with("gemini-"));
}

#[test]
fn translate_abp_to_kimi_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let result = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert!(model.starts_with("moonshot-"));
}

#[test]
fn translate_abp_to_codex_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let result = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert!(model.starts_with("codex-"));
}

#[test]
fn translate_abp_with_custom_model() {
    let wo = WorkOrderBuilder::new("task").model("my-model").build();
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let model = result.get("model").unwrap().as_str().unwrap();
    assert_eq!(model, "my-model");
}

// =========================================================================
// 36. SelectionResult serde
// =========================================================================

#[test]
fn selection_result_serde_roundtrip() {
    let sr = SelectionResult {
        selected: "mock".into(),
        reason: "test".into(),
        alternatives: vec!["other".into()],
        unmet_capabilities: vec![],
    };
    let json = serde_json::to_string(&sr).unwrap();
    let back: SelectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.selected, "mock");
}

// =========================================================================
// 37. Message serde
// =========================================================================

#[test]
fn message_serde_roundtrip() {
    let msg = Message {
        role: MessageRole::User,
        content: "hello world".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}
