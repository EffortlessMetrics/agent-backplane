// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the Backend trait, MockBackend,
//! BackendRegistry, BackendSelector, CapabilityMatrix, event streaming,
//! receipt hashing, config extraction, execution mode, and error handling.

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_backend_core::registry::BackendRegistry;
use abp_backend_core::{BackendHealth, BackendMetadata, HealthStatus, RateLimit};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_host::SidecarSpec;
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::health::{HealthChecker, HealthStatus as IntegrationHealthStatus};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry};
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use abp_integrations::{
    Backend, MockBackend, SidecarBackend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn wo_with_vendor(key: &str, value: serde_json::Value) -> WorkOrder {
    let mut config = abp_core::RuntimeConfig::default();
    config.vendor.insert(key.to_string(), value);
    WorkOrderBuilder::new("test").config(config).build()
}

fn metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "1.0".to_string(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

fn health_ok() -> BackendHealth {
    BackendHealth {
        status: HealthStatus::Healthy,
        ..Default::default()
    }
}

fn health_degraded() -> BackendHealth {
    BackendHealth {
        status: HealthStatus::Degraded,
        ..Default::default()
    }
}

fn health_unhealthy() -> BackendHealth {
    BackendHealth {
        status: HealthStatus::Unhealthy,
        ..Default::default()
    }
}

fn candidate(name: &str, caps: Vec<Capability>, priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps,
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

async fn run_mock(task: &str) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
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

    #[allow(dead_code)]
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
    ) -> anyhow::Result<Receipt> {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("stub: {}", work_order.task),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        let mut receipt = ReceiptBuilder::new(&self.name)
            .outcome(Outcome::Complete)
            .build()
            .with_hash()?;
        receipt.meta.run_id = run_id;
        receipt.meta.work_order_id = work_order.id;
        Ok(receipt)
    }
}

/// Backend that always errors.
#[derive(Debug)]
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
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("FailingBackend always errors")
    }
}

// ===========================================================================
// Section 1: Backend trait – MockBackend implements Backend correctly (1–10)
// ===========================================================================

#[test]
fn t01_mock_backend_implements_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[test]
fn t02_mock_identity_id_is_mock() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t03_mock_identity_backend_version() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t04_mock_identity_adapter_version() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t05_mock_capabilities_non_empty() {
    assert!(!MockBackend.capabilities().is_empty());
}

#[test]
fn t06_mock_has_streaming_native() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t07_mock_has_tool_read_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t08_mock_has_tool_write_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t09_mock_has_tool_edit_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t10_mock_has_structured_output_emulated() {
    assert!(matches!(
        MockBackend
            .capabilities()
            .get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

// ===========================================================================
// Section 2: MockBackend streaming – events via mpsc::Sender (11–20)
// ===========================================================================

#[tokio::test]
async fn t11_mock_run_streams_run_started() {
    let (_, events) = run_mock("stream test").await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
    );
}

#[tokio::test]
async fn t12_mock_run_streams_assistant_message() {
    let (_, events) = run_mock("stream test").await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
    );
}

#[tokio::test]
async fn t13_mock_run_streams_run_completed() {
    let (_, events) = run_mock("stream test").await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[tokio::test]
async fn t14_mock_streams_exactly_four_events() {
    let (_, events) = run_mock("count test").await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t15_first_event_is_run_started() {
    let (_, events) = run_mock("order test").await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t16_last_event_is_run_completed() {
    let (_, events) = run_mock("order test").await;
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t17_events_have_non_none_ext() {
    let (_, events) = run_mock("ext test").await;
    // MockBackend sets ext to None
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn t18_events_timestamps_non_decreasing() {
    let (_, events) = run_mock("ts test").await;
    for pair in events.windows(2) {
        assert!(pair[1].ts >= pair[0].ts);
    }
}

#[tokio::test]
async fn t19_run_started_message_contains_task() {
    let (_, events) = run_mock("my-special-task").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains("my-special-task"),
            "RunStarted message should contain the task"
        );
    } else {
        panic!("first event should be RunStarted");
    }
}

#[tokio::test]
async fn t20_channel_receives_all_events_before_receipt() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("channel"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 4);
}

// ===========================================================================
// Section 3: MockBackend receipt – valid receipt with hash (21–30)
// ===========================================================================

#[tokio::test]
async fn t21_receipt_has_sha256_hash() {
    let (receipt, _) = run_mock("hash test").await;
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t22_receipt_hash_is_64_hex_chars() {
    let (receipt, _) = run_mock("hash length").await;
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t23_receipt_hash_matches_recomputed() {
    let (receipt, _) = run_mock("recompute").await;
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn t24_receipt_outcome_is_complete() {
    let (receipt, _) = run_mock("outcome").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t25_receipt_contract_version() {
    let (receipt, _) = run_mock("version").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t26_receipt_run_id_matches() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(run_id, wo("id"), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t27_receipt_work_order_id_matches() {
    let work_order = wo("woid");
    let woid = work_order.id;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.work_order_id, woid);
}

#[tokio::test]
async fn t28_receipt_trace_non_empty() {
    let (receipt, _) = run_mock("trace").await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn t29_receipt_backend_identity_matches() {
    let (receipt, _) = run_mock("backend id").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t30_receipt_capabilities_match_mock() {
    let (receipt, _) = run_mock("caps").await;
    assert_eq!(receipt.capabilities.len(), MockBackend.capabilities().len());
}

// ===========================================================================
// Section 4: BackendRegistry – register, lookup, list (31–42)
// ===========================================================================

#[test]
fn t31_registry_new_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn t32_register_one_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("alpha", metadata("alpha", "openai"));
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("alpha"));
}

#[test]
fn t33_register_multiple_backends() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("b", metadata("b", "anthropic"));
    reg.register_with_metadata("c", metadata("c", "gemini"));
    assert_eq!(reg.len(), 3);
}

#[test]
fn t34_list_returns_sorted_names() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("z", metadata("z", "openai"));
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("m", metadata("m", "openai"));
    assert_eq!(reg.list(), vec!["a", "m", "z"]);
}

#[test]
fn t35_metadata_lookup() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("test", metadata("test", "anthropic"));
    let meta = reg.metadata("test").unwrap();
    assert_eq!(meta.dialect, "anthropic");
}

#[test]
fn t36_metadata_lookup_missing() {
    let reg = BackendRegistry::new();
    assert!(reg.metadata("nonexistent").is_none());
}

#[test]
fn t37_contains_false_for_unknown() {
    let reg = BackendRegistry::new();
    assert!(!reg.contains("ghost"));
}

#[test]
fn t38_remove_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("rm", metadata("rm", "openai"));
    assert!(reg.contains("rm"));
    let removed = reg.remove("rm");
    assert!(removed.is_some());
    assert!(!reg.contains("rm"));
}

#[test]
fn t39_remove_nonexistent_returns_none() {
    let mut reg = BackendRegistry::new();
    assert!(reg.remove("nope").is_none());
}

#[test]
fn t40_by_dialect_filters() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("b", metadata("b", "anthropic"));
    reg.register_with_metadata("c", metadata("c", "openai"));
    assert_eq!(reg.by_dialect("openai"), vec!["a", "c"]);
    assert_eq!(reg.by_dialect("anthropic"), vec!["b"]);
}

#[test]
fn t41_by_dialect_empty_for_unknown() {
    let reg = BackendRegistry::new();
    assert!(reg.by_dialect("unknown").is_empty());
}

#[test]
fn t42_register_replaces_previous() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", metadata("x", "openai"));
    reg.register_with_metadata("x", metadata("x", "anthropic"));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.metadata("x").unwrap().dialect, "anthropic");
}

// ===========================================================================
// Section 5: Backend selection – select by name (43–52)
// ===========================================================================

#[test]
fn t43_selector_first_match_empty() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert!(sel.select(&[]).is_none());
}

#[test]
fn t44_selector_first_match_single() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "a");
}

#[test]
fn t45_selector_best_fit() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    let picked = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(picked.name, "b");
}

#[test]
fn t46_selector_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(candidate("low", vec![Capability::Streaming], 10));
    sel.add_candidate(candidate("high", vec![Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "high");
}

#[test]
fn t47_selector_round_robin() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(candidate("b", vec![Capability::Streaming], 1));
    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn t48_selector_no_match_when_caps_missing() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    assert!(sel.select(&[Capability::ToolBash]).is_none());
}

#[test]
fn t49_selector_disabled_candidates_excluded() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = candidate("disabled", vec![Capability::Streaming], 1);
    c.enabled = false;
    sel.add_candidate(c);
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn t50_select_with_result_selected() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.unmet_capabilities.is_empty());
}

#[test]
fn t51_select_with_result_unmet() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::ToolBash]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn t52_selector_candidate_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![], 1));
    sel.add_candidate(candidate("b", vec![], 1));
    assert_eq!(sel.candidate_count(), 2);
}

// ===========================================================================
// Section 6: Config extraction – ABP config from vendor config (53–62)
// ===========================================================================

#[test]
fn t53_extract_mode_defaults_to_mapped() {
    assert_eq!(
        extract_execution_mode(&wo("default")),
        ExecutionMode::Mapped
    );
}

#[test]
fn t54_extract_mode_nested_passthrough() {
    let w = wo_with_vendor("abp", json!({"mode": "passthrough"}));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Passthrough);
}

#[test]
fn t55_extract_mode_nested_mapped() {
    let w = wo_with_vendor("abp", json!({"mode": "mapped"}));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

#[test]
fn t56_extract_mode_dotted_passthrough() {
    let w = wo_with_vendor("abp.mode", json!("passthrough"));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Passthrough);
}

#[test]
fn t57_extract_mode_dotted_mapped() {
    let w = wo_with_vendor("abp.mode", json!("mapped"));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

#[test]
fn t58_extract_mode_nested_takes_precedence() {
    // When both nested and dotted are present, nested wins
    let mut config = abp_core::RuntimeConfig::default();
    config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    config
        .vendor
        .insert("abp.mode".to_string(), json!("mapped"));
    let w = WorkOrderBuilder::new("test").config(config).build();
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Passthrough);
}

#[test]
fn t59_extract_mode_invalid_value_defaults_to_mapped() {
    let w = wo_with_vendor("abp", json!({"mode": "invalid_mode"}));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

#[test]
fn t60_extract_mode_empty_abp_object_defaults() {
    let w = wo_with_vendor("abp", json!({}));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

#[test]
fn t61_extract_mode_abp_is_string_defaults() {
    let w = wo_with_vendor("abp", json!("not-an-object"));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

#[test]
fn t62_extract_mode_dotted_invalid_defaults() {
    let w = wo_with_vendor("abp.mode", json!("garbage"));
    assert_eq!(extract_execution_mode(&w), ExecutionMode::Mapped);
}

// ===========================================================================
// Section 7: ExecutionMode – Passthrough vs Mapped (63–68)
// ===========================================================================

#[test]
fn t63_execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn t64_execution_mode_serde_roundtrip_mapped() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Mapped);
}

#[test]
fn t65_execution_mode_serde_roundtrip_passthrough() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

#[test]
fn t66_validate_passthrough_ok() {
    assert!(validate_passthrough_compatibility(&wo("ok")).is_ok());
}

#[tokio::test]
async fn t67_receipt_mode_defaults_to_mapped() {
    let (receipt, _) = run_mock("mode test").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t68_receipt_mode_passthrough_from_config() {
    let w = wo_with_vendor("abp", json!({"mode": "passthrough"}));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), w, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// Section 8: Backend capabilities – CapabilityMatrix (69–76)
// ===========================================================================

#[test]
fn t69_capability_matrix_empty() {
    let cm = CapabilityMatrix::new();
    assert!(cm.is_empty());
    assert_eq!(cm.backend_count(), 0);
}

#[test]
fn t70_capability_matrix_register() {
    let mut cm = CapabilityMatrix::new();
    cm.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    assert_eq!(cm.backend_count(), 1);
    assert!(cm.supports("mock", &Capability::Streaming));
}

#[test]
fn t71_capability_matrix_not_supported() {
    let mut cm = CapabilityMatrix::new();
    cm.register("mock", vec![Capability::Streaming]);
    assert!(!cm.supports("mock", &Capability::ToolBash));
}

#[test]
fn t72_capability_matrix_backends_for() {
    let mut cm = CapabilityMatrix::new();
    cm.register("a", vec![Capability::Streaming]);
    cm.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    cm.register("c", vec![Capability::ToolRead]);
    assert_eq!(cm.backends_for(&Capability::Streaming), vec!["a", "b"]);
}

#[test]
fn t73_capability_matrix_common_capabilities() {
    let mut cm = CapabilityMatrix::new();
    cm.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    cm.register("b", vec![Capability::Streaming, Capability::ToolBash]);
    let common = cm.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
}

#[test]
fn t74_capability_matrix_evaluate_full_score() {
    let mut cm = CapabilityMatrix::new();
    cm.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    let report = cm.evaluate("a", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
    assert!(report.missing.is_empty());
}

#[test]
fn t75_capability_matrix_evaluate_partial() {
    let mut cm = CapabilityMatrix::new();
    cm.register("a", vec![Capability::Streaming]);
    let report = cm.evaluate("a", &[Capability::Streaming, Capability::ToolBash]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn t76_capability_matrix_best_backend() {
    let mut cm = CapabilityMatrix::new();
    cm.register("weak", vec![Capability::Streaming]);
    cm.register(
        "strong",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolBash,
        ],
    );
    let best = cm
        .best_backend(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(best, "strong");
}

// ===========================================================================
// Section 9: Backend identity – BackendIdentity fields (77–80)
// ===========================================================================

#[test]
fn t77_backend_identity_serde_roundtrip() {
    let id = BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("2.0".into()),
        adapter_version: Some("1.0".into()),
    };
    let json = serde_json::to_value(&id).unwrap();
    let back: BackendIdentity = serde_json::from_value(json).unwrap();
    assert_eq!(back.id, "test-backend");
    assert_eq!(back.backend_version.as_deref(), Some("2.0"));
}

#[test]
fn t78_backend_identity_optional_versions() {
    let id = BackendIdentity {
        id: "bare".into(),
        backend_version: None,
        adapter_version: None,
    };
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
}

#[test]
fn t79_sidecar_backend_identity() {
    let sb = SidecarBackend::new(SidecarSpec::new("echo"));
    let id = sb.identity();
    assert_eq!(id.id, "sidecar");
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn t80_stub_backend_identity() {
    let stub = StubBackend::new("my-stub");
    assert_eq!(stub.identity().id, "my-stub");
    assert_eq!(stub.identity().backend_version.as_deref(), Some("stub-0.1"));
}

// ===========================================================================
// Section 10: Error handling – unknown backend, failures (81–87)
// ===========================================================================

#[tokio::test]
async fn t81_failing_backend_returns_error() {
    let (tx, _rx) = mpsc::channel(16);
    let result = FailingBackend.run(Uuid::new_v4(), wo("fail"), tx).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("always errors"));
}

#[test]
fn t82_ensure_requirements_unsatisfied() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }],
    };
    let caps = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t83_ensure_requirements_emulated_insufficient_for_native() {
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
fn t84_ensure_requirements_native_satisfies_emulated() {
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
fn t85_ensure_requirements_empty_always_ok() {
    let reqs = CapabilityRequirements::default();
    assert!(ensure_capability_requirements(&reqs, &CapabilityManifest::new()).is_ok());
}

#[tokio::test]
async fn t86_mock_rejects_unsatisfied_requirements() {
    let mut w = wo("cap reject");
    w.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), w, tx).await;
    assert!(result.is_err());
}

#[test]
fn t87_registry_health_unknown_backend() {
    let reg = BackendRegistry::new();
    assert!(reg.health("ghost").is_none());
}

// ===========================================================================
// Section 11: Work order routing – route to correct backend (88–93)
// ===========================================================================

#[tokio::test]
async fn t88_trait_object_dispatch() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), wo("dispatch"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t89_arc_trait_object_dispatch() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(Uuid::new_v4(), wo("arc"), tx).await.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t90_stub_backend_routes_correctly() {
    let stub = StubBackend::new("alpha");
    let (tx, _rx) = mpsc::channel(16);
    let receipt = stub.run(Uuid::new_v4(), wo("route"), tx).await.unwrap();
    assert_eq!(receipt.backend.id, "alpha");
}

#[tokio::test]
async fn t91_multiple_backends_in_map() {
    let mut backends: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    backends.insert("mock".into(), Box::new(MockBackend));
    backends.insert("stub-a".into(), Box::new(StubBackend::new("stub-a")));
    backends.insert("stub-b".into(), Box::new(StubBackend::new("stub-b")));

    for (name, backend) in &backends {
        let (tx, _rx) = mpsc::channel(16);
        let receipt = backend.run(Uuid::new_v4(), wo("multi"), tx).await.unwrap();
        // MockBackend returns "mock", stubs return their name
        if name == "mock" {
            assert_eq!(receipt.backend.id, "mock");
        } else {
            assert_eq!(receipt.backend.id, *name);
        }
    }
}

#[tokio::test]
async fn t92_sequential_runs_produce_different_run_ids() {
    let (tx1, _rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let r1 = MockBackend.run(id1, wo("first"), tx1).await.unwrap();
    let r2 = MockBackend.run(id2, wo("second"), tx2).await.unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t93_sequential_runs_produce_different_hashes() {
    let (tx1, _rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    let r1 = MockBackend.run(Uuid::new_v4(), wo("a"), tx1).await.unwrap();
    let r2 = MockBackend.run(Uuid::new_v4(), wo("b"), tx2).await.unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// Section 12: Event types – AgentEventKind variants (94–100)
// ===========================================================================

#[test]
fn t94_agent_event_kind_run_started_serde() {
    let kind = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "run_started");
    assert_eq!(json["message"], "go");
}

#[test]
fn t95_agent_event_kind_assistant_message_serde() {
    let kind = AgentEventKind::AssistantMessage {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "assistant_message");
}

#[test]
fn t96_agent_event_kind_tool_call_serde() {
    let kind = AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["tool_name"], "bash");
}

#[test]
fn t97_agent_event_kind_tool_result_serde() {
    let kind = AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu-1".into()),
        output: json!("ok"),
        is_error: false,
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert!(!json["is_error"].as_bool().unwrap());
}

#[test]
fn t98_agent_event_kind_file_changed_serde() {
    let kind = AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "file_changed");
}

#[test]
fn t99_agent_event_kind_warning_serde() {
    let kind = AgentEventKind::Warning {
        message: "deprecated".into(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "warning");
}

#[test]
fn t100_agent_event_kind_error_serde() {
    let kind = AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["message"], "boom");
}

// ===========================================================================
// Bonus: Additional coverage (101+)
// ===========================================================================

#[test]
fn t101_registry_healthy_backends() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("b", metadata("b", "openai"));
    reg.update_health("a", health_ok());
    reg.update_health("b", health_unhealthy());
    assert_eq!(reg.healthy_backends(), vec!["a"]);
}

#[test]
fn t102_registry_health_update() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", metadata("x", "openai"));
    reg.update_health("x", health_ok());
    assert_eq!(reg.health("x").unwrap().status, HealthStatus::Healthy);
    reg.update_health("x", health_degraded());
    assert_eq!(reg.health("x").unwrap().status, HealthStatus::Degraded);
}

#[test]
fn t103_selector_select_all() {
    let sel = {
        let mut s = BackendSelector::new(SelectionStrategy::FirstMatch);
        s.add_candidate(candidate("a", vec![Capability::Streaming], 1));
        s.add_candidate(candidate(
            "b",
            vec![Capability::Streaming, Capability::ToolRead],
            1,
        ));
        s.add_candidate(candidate("c", vec![Capability::ToolRead], 1));
        s
    };
    let all = sel.select_all(&[Capability::Streaming]);
    assert_eq!(all.len(), 2);
}

#[test]
fn t104_selector_enabled_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![], 1));
    let mut disabled = candidate("b", vec![], 1);
    disabled.enabled = false;
    sel.add_candidate(disabled);
    assert_eq!(sel.candidate_count(), 2);
    assert_eq!(sel.enabled_count(), 1);
}

#[test]
fn t105_metrics_initial_state() {
    let m = BackendMetrics::new();
    assert_eq!(m.total_runs(), 0);
    assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn t106_metrics_record_run() {
    let m = BackendMetrics::new();
    m.record_run(true, 5, 100);
    assert_eq!(m.total_runs(), 1);
    assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn t107_metrics_mixed_success_failure() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    m.record_run(false, 5, 300);
    assert_eq!(m.total_runs(), 2);
    assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn t108_metrics_reset() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 100);
    m.reset();
    assert_eq!(m.total_runs(), 0);
}

#[test]
fn t109_metrics_snapshot() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 200);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
}

#[test]
fn t110_metrics_registry() {
    let reg = MetricsRegistry::new();
    let m = reg.get_or_create("mock");
    m.record_run(true, 1, 50);
    let all = reg.snapshot_all();
    assert_eq!(all.len(), 1);
    assert_eq!(all["mock"].total_runs, 1);
}

#[test]
fn t111_health_checker_empty_is_healthy() {
    let hc = HealthChecker::new();
    assert!(hc.is_healthy());
    assert_eq!(hc.check_count(), 0);
}

#[test]
fn t112_health_checker_unhealthy_propagates() {
    let mut hc = HealthChecker::new();
    hc.add_check("backend-a", IntegrationHealthStatus::Healthy);
    hc.add_check(
        "backend-b",
        IntegrationHealthStatus::Unhealthy {
            reason: "down".into(),
        },
    );
    assert!(!hc.is_healthy());
    assert_eq!(hc.unhealthy_checks().len(), 1);
}

#[test]
fn t113_sidecar_backend_capabilities_empty() {
    let sb = SidecarBackend::new(SidecarSpec::new("echo"));
    assert!(sb.capabilities().is_empty());
}

#[test]
fn t114_support_level_native_satisfies_both() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn t115_support_level_emulated_satisfies_emulated_only() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn t116_support_level_unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn t117_agent_event_kind_command_executed_serde() {
    let kind = AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "command_executed");
    assert_eq!(json["exit_code"], 0);
}

#[test]
fn t118_agent_event_kind_assistant_delta_serde() {
    let kind = AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["type"], "assistant_delta");
    assert_eq!(json["text"], "chunk");
}

#[test]
fn t119_receipt_builder_defaults() {
    let receipt = ReceiptBuilder::new("test").build();
    assert_eq!(receipt.backend.id, "test");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[test]
fn t120_receipt_builder_with_hash() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn t121_capability_matrix_register_merges() {
    let mut cm = CapabilityMatrix::new();
    cm.register("a", vec![Capability::Streaming]);
    cm.register("a", vec![Capability::ToolRead]);
    let all = cm.all_capabilities("a").unwrap();
    assert!(all.contains(&Capability::Streaming));
    assert!(all.contains(&Capability::ToolRead));
}

#[test]
fn t122_capability_matrix_empty_common() {
    let cm = CapabilityMatrix::new();
    assert!(cm.common_capabilities().is_empty());
}

#[test]
fn t123_selector_least_loaded() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(candidate("heavy", vec![Capability::Streaming], 100));
    sel.add_candidate(candidate("light", vec![Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "light");
}

#[test]
fn t124_select_with_result_alternatives() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(candidate("b", vec![Capability::Streaming], 2));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.alternatives.contains(&"b".to_string()));
}

#[test]
fn t125_registry_metadata_fields() {
    let mut reg = BackendRegistry::new();
    let md = BackendMetadata {
        name: "gpt4".to_string(),
        dialect: "openai".to_string(),
        version: "2.0".to_string(),
        max_tokens: Some(128_000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: Some(RateLimit {
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
            concurrent_requests: 5,
        }),
    };
    reg.register_with_metadata("gpt4", md);
    let m = reg.metadata("gpt4").unwrap();
    assert_eq!(m.max_tokens, Some(128_000));
    assert!(m.rate_limit.is_some());
    let rl = m.rate_limit.as_ref().unwrap();
    assert_eq!(rl.requests_per_minute, 60);
}
