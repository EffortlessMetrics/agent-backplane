// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for BackendRegistry, Backend trait, MockBackend, SidecarBackend,
//! and integration patterns. Covers registry CRUD, trait object safety, event
//! streaming, receipt hashing, thread safety, health tracking, and selection.

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

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
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

fn health_with_status(status: HealthStatus) -> BackendHealth {
    BackendHealth {
        status,
        ..Default::default()
    }
}

/// Minimal custom backend for trait-object and registry tests.
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
    ) -> anyhow::Result<Receipt> {
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("stub[{}]: {}", self.name, work_order.task),
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
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

// ===========================================================================
// 1. BackendRegistry — register and lookup (tests 1–15)
// ===========================================================================

#[test]
fn t01_registry_new_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn t02_registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn t03_register_single_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", metadata("mock", "mock"));
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("mock"));
}

#[test]
fn t04_lookup_returns_metadata() {
    let mut reg = BackendRegistry::new();
    let m = metadata("mock", "mock");
    reg.register_with_metadata("mock", m.clone());
    let found = reg.metadata("mock").unwrap();
    assert_eq!(found.name, "mock");
    assert_eq!(found.dialect, "mock");
}

#[test]
fn t05_lookup_nonexistent_returns_none() {
    let reg = BackendRegistry::new();
    assert!(reg.metadata("missing").is_none());
}

#[test]
fn t06_register_multiple_backends() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("b", metadata("b", "anthropic"));
    reg.register_with_metadata("c", metadata("c", "gemini"));
    assert_eq!(reg.len(), 3);
}

#[test]
fn t07_list_returns_sorted_names() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("zulu", metadata("zulu", "z"));
    reg.register_with_metadata("alpha", metadata("alpha", "a"));
    reg.register_with_metadata("mike", metadata("mike", "m"));
    assert_eq!(reg.list(), vec!["alpha", "mike", "zulu"]);
}

#[test]
fn t08_duplicate_registration_replaces() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", metadata("x", "old"));
    reg.register_with_metadata("x", metadata("x", "new"));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.metadata("x").unwrap().dialect, "new");
}

#[test]
fn t09_contains_false_for_missing() {
    let reg = BackendRegistry::new();
    assert!(!reg.contains("nope"));
}

#[test]
fn t10_remove_existing_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("rm", metadata("rm", "d"));
    let removed = reg.remove("rm");
    assert!(removed.is_some());
    assert!(reg.is_empty());
}

#[test]
fn t11_remove_nonexistent_returns_none() {
    let mut reg = BackendRegistry::new();
    assert!(reg.remove("ghost").is_none());
}

#[test]
fn t12_health_default_on_register() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("h", metadata("h", "d"));
    let health = reg.health("h").unwrap();
    assert_eq!(health.status, HealthStatus::Unknown);
}

#[test]
fn t13_update_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("h", metadata("h", "d"));
    reg.update_health("h", health_with_status(HealthStatus::Healthy));
    assert_eq!(reg.health("h").unwrap().status, HealthStatus::Healthy);
}

#[test]
fn t14_healthy_backends_filter() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "d"));
    reg.register_with_metadata("b", metadata("b", "d"));
    reg.update_health("a", health_with_status(HealthStatus::Healthy));
    reg.update_health("b", health_with_status(HealthStatus::Unhealthy));
    let healthy = reg.healthy_backends();
    assert_eq!(healthy, vec!["a"]);
}

#[test]
fn t15_by_dialect_filter() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", metadata("a", "openai"));
    reg.register_with_metadata("b", metadata("b", "anthropic"));
    reg.register_with_metadata("c", metadata("c", "openai"));
    let openai = reg.by_dialect("openai");
    assert_eq!(openai, vec!["a", "c"]);
}

// ===========================================================================
// 2. BackendRegistry — thread safety via Arc (tests 16–20)
// ===========================================================================

#[test]
fn t16_registry_is_clone() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", metadata("x", "d"));
    let reg2 = reg.clone();
    assert_eq!(reg2.len(), 1);
}

#[test]
fn t17_registry_debug_impl() {
    let reg = BackendRegistry::new();
    let dbg = format!("{reg:?}");
    assert!(dbg.contains("BackendRegistry"));
}

#[test]
fn t18_registry_arc_wrapping() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("arc", metadata("arc", "d"));
    let arc_reg = Arc::new(reg);
    assert_eq!(arc_reg.len(), 1);
    assert!(arc_reg.contains("arc"));
}

#[test]
fn t19_registry_arc_clone_shares_snapshot() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("shared", metadata("shared", "d"));
    let a = Arc::new(reg);
    let b = Arc::clone(&a);
    assert_eq!(a.len(), b.len());
}

#[test]
fn t20_registry_remove_also_removes_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gone", metadata("gone", "d"));
    reg.update_health("gone", health_with_status(HealthStatus::Healthy));
    reg.remove("gone");
    assert!(reg.health("gone").is_none());
}

// ===========================================================================
// 3. MockBackend — identity, capabilities, events (tests 21–35)
// ===========================================================================

#[test]
fn t21_mock_identity_id() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t22_mock_identity_backend_version() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t23_mock_identity_adapter_version() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t24_mock_capabilities_streaming_native() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t25_mock_capabilities_tool_read_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t26_mock_capabilities_tool_write_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t27_mock_capabilities_tool_edit_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t28_mock_capabilities_tool_bash_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t29_mock_capabilities_json_schema_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t30_mock_capabilities_count() {
    let caps = MockBackend.capabilities();
    assert_eq!(caps.len(), 6);
}

#[tokio::test]
async fn t31_mock_run_produces_receipt() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("hello"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t32_mock_run_receipt_has_hash() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("hash-test"), tx)
        .await
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t33_mock_run_receipt_hash_verifiable() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("verify"), tx)
        .await
        .unwrap();
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn t34_mock_run_streams_events() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("events"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 3, "expected at least 3 events, got {count}");
}

#[tokio::test]
async fn t35_mock_run_event_sequence() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("seq"), tx)
        .await
        .unwrap();
    let mut kinds = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        kinds.push(ev.kind);
    }
    assert!(matches!(
        kinds.first(),
        Some(AgentEventKind::RunStarted { .. })
    ));
    assert!(matches!(
        kinds.last(),
        Some(AgentEventKind::RunCompleted { .. })
    ));
}

// ===========================================================================
// 4. MockBackend — receipt details & empty events (tests 36–42)
// ===========================================================================

#[tokio::test]
async fn t36_mock_receipt_contract_version() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo("cv"), tx).await.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t37_mock_receipt_run_id_matches() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(run_id, wo("rid"), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t38_mock_receipt_work_order_id_matches() {
    let order = wo("woid");
    let expected_id = order.id;
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, expected_id);
}

#[tokio::test]
async fn t39_mock_receipt_trace_nonempty() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("trace"), tx)
        .await
        .unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn t40_mock_receipt_backend_identity() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo("id"), tx).await.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t41_mock_receipt_mode_default_mapped() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("mode"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t42_mock_receipt_harness_ok() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("harness"), tx)
        .await
        .unwrap();
    assert!(receipt.verification.harness_ok);
}

// ===========================================================================
// 5. MockBackend — streaming via mpsc (tests 43–47)
// ===========================================================================

#[tokio::test]
async fn t43_mock_channel_small_buffer() {
    // Use a small (but sufficient) buffer; capacity=1 would deadlock because
    // MockBackend::run sends multiple events synchronously.
    let (tx, mut rx) = mpsc::channel(4);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("cap1"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 1);
}

#[tokio::test]
async fn t44_mock_events_contain_assistant_message() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("msg"), tx)
        .await
        .unwrap();
    let mut found = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev.kind, AgentEventKind::AssistantMessage { .. }) {
            found = true;
        }
    }
    assert!(found, "expected at least one AssistantMessage event");
}

#[tokio::test]
async fn t45_mock_events_no_ext_fields() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("ext"), tx)
        .await
        .unwrap();
    while let Ok(ev) = rx.try_recv() {
        assert!(ev.ext.is_none(), "mock events should not set ext");
    }
}

#[tokio::test]
async fn t46_mock_sequential_runs_independent() {
    let (tx1, _rx1) = mpsc::channel(64);
    let (tx2, _rx2) = mpsc::channel(64);
    let r1 = MockBackend
        .run(Uuid::new_v4(), wo("r1"), tx1)
        .await
        .unwrap();
    let r2 = MockBackend
        .run(Uuid::new_v4(), wo("r2"), tx2)
        .await
        .unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t47_mock_concurrent_runs() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(64);
                b.run(Uuid::new_v4(), wo(&format!("concurrent-{i}")), tx)
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut run_ids = Vec::new();
    for h in handles {
        let receipt = h.await.unwrap();
        run_ids.push(receipt.meta.run_id);
    }
    // All run IDs should be unique.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 5);
}

// ===========================================================================
// 6. Backend trait contract (tests 48–55)
// ===========================================================================

#[test]
fn t48_mock_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MockBackend>();
}

#[test]
fn t49_mock_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<MockBackend>();
}

#[test]
fn t50_sidecar_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<SidecarBackend>();
}

#[test]
fn t51_sidecar_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<SidecarBackend>();
}

#[test]
fn t52_dyn_backend_is_object_safe() {
    let _: Box<dyn Backend> = Box::new(MockBackend);
}

#[test]
fn t53_stub_backend_as_trait_object() {
    let b: Box<dyn Backend> = Box::new(StubBackend::new("obj"));
    assert_eq!(b.identity().id, "obj");
}

#[tokio::test]
async fn t54_trait_object_run() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(64);
    let receipt = b.run(Uuid::new_v4(), wo("dyn"), tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn t55_trait_object_in_vec() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(MockBackend),
        Box::new(StubBackend::new("s1")),
        Box::new(StubBackend::new("s2")),
    ];
    assert_eq!(backends.len(), 3);
    assert_eq!(backends[0].identity().id, "mock");
    assert_eq!(backends[1].identity().id, "s1");
}

// ===========================================================================
// 7. SidecarBackend — unit construction (tests 56–62)
// ===========================================================================

#[test]
fn t56_sidecar_new_with_spec() {
    let spec = SidecarSpec::new("echo");
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.command, "echo");
}

#[test]
fn t57_sidecar_identity_type() {
    let sb = SidecarBackend::new(SidecarSpec::new("cmd"));
    assert_eq!(sb.identity().id, "sidecar");
}

#[test]
fn t58_sidecar_identity_adapter_version() {
    let sb = SidecarBackend::new(SidecarSpec::new("cmd"));
    assert_eq!(sb.identity().adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn t59_sidecar_identity_no_backend_version() {
    let sb = SidecarBackend::new(SidecarSpec::new("cmd"));
    assert!(sb.identity().backend_version.is_none());
}

#[test]
fn t60_sidecar_capabilities_empty() {
    let sb = SidecarBackend::new(SidecarSpec::new("cmd"));
    assert!(sb.capabilities().is_empty());
}

#[test]
fn t61_sidecar_clone() {
    let sb = SidecarBackend::new(SidecarSpec::new("cloneme"));
    let sb2 = sb.clone();
    assert_eq!(sb2.spec.command, "cloneme");
}

#[test]
fn t62_sidecar_debug() {
    let sb = SidecarBackend::new(SidecarSpec::new("dbg"));
    let dbg = format!("{sb:?}");
    assert!(dbg.contains("SidecarBackend"));
}

// ===========================================================================
// 8. SidecarBackend — spec configuration (tests 63–67)
// ===========================================================================

#[test]
fn t63_sidecar_spec_with_args() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".to_string()];
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.args, vec!["index.js"]);
}

#[test]
fn t64_sidecar_spec_with_env() {
    let mut spec = SidecarSpec::new("node");
    spec.env.insert("API_KEY".to_string(), "secret".to_string());
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.env.get("API_KEY").unwrap(), "secret");
}

#[test]
fn t65_sidecar_spec_with_cwd() {
    let mut spec = SidecarSpec::new("python");
    spec.cwd = Some("/tmp/workspace".to_string());
    let sb = SidecarBackend::new(spec);
    assert_eq!(sb.spec.cwd.as_deref(), Some("/tmp/workspace"));
}

#[test]
fn t66_sidecar_spec_empty_args() {
    let spec = SidecarSpec::new("echo");
    assert!(spec.args.is_empty());
}

#[test]
fn t67_sidecar_spec_empty_env() {
    let spec = SidecarSpec::new("echo");
    assert!(spec.env.is_empty());
}

// ===========================================================================
// 9. ensure_capability_requirements (tests 68–73)
// ===========================================================================

#[test]
fn t68_empty_requirements_pass() {
    let reqs = CapabilityRequirements::default();
    let caps = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t69_satisfied_requirement() {
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
fn t70_unsatisfied_requirement_fails() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let caps = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t71_multiple_requirements_all_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t72_partial_requirements_fails() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            },
        ],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t73_passthrough_compatibility_always_ok() {
    let order = wo("pass");
    assert!(validate_passthrough_compatibility(&order).is_ok());
}

// ===========================================================================
// 10. extract_execution_mode (tests 74–77)
// ===========================================================================

#[test]
fn t74_default_mode_is_mapped() {
    let order = wo("mode");
    assert_eq!(extract_execution_mode(&order), ExecutionMode::Mapped);
}

#[test]
fn t75_nested_passthrough_mode() {
    let mut order = wo("mode");
    order.config.vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    assert_eq!(extract_execution_mode(&order), ExecutionMode::Passthrough);
}

#[test]
fn t76_dotted_passthrough_mode() {
    let mut order = wo("mode");
    order
        .config
        .vendor
        .insert("abp.mode".to_string(), serde_json::json!("passthrough"));
    assert_eq!(extract_execution_mode(&order), ExecutionMode::Passthrough);
}

#[test]
fn t77_invalid_mode_falls_back_to_mapped() {
    let mut order = wo("mode");
    order.config.vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "invalid_mode"}),
    );
    assert_eq!(extract_execution_mode(&order), ExecutionMode::Mapped);
}

// ===========================================================================
// 11. ErrorBackend & error simulation (tests 78–80)
// ===========================================================================

#[tokio::test]
async fn t78_error_backend_run_fails() {
    let b = ErrorBackend {
        message: "boom".into(),
    };
    let (tx, _rx) = mpsc::channel(64);
    let result = b.run(Uuid::new_v4(), wo("fail"), tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn t79_error_backend_message_propagated() {
    let b = ErrorBackend {
        message: "specific error message".into(),
    };
    let (tx, _rx) = mpsc::channel(64);
    let err = b.run(Uuid::new_v4(), wo("fail"), tx).await.unwrap_err();
    assert!(err.to_string().contains("specific error message"));
}

#[tokio::test]
async fn t80_error_backend_identity() {
    let b = ErrorBackend {
        message: "x".into(),
    };
    assert_eq!(b.identity().id, "error");
}

// ===========================================================================
// 12. Integration patterns — selection (tests 81–90)
// ===========================================================================

#[test]
fn t81_selector_first_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "a");
}

#[test]
fn t82_selector_no_match_returns_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    assert!(sel.select(&[Capability::McpClient]).is_none());
}

#[test]
fn t83_selector_best_fit() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(BackendCandidate {
        name: "partial".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "full".into(),
        capabilities: vec![Capability::Streaming, Capability::ToolRead],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let picked = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(picked.name, "full");
}

#[test]
fn t84_selector_round_robin() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "b".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn t85_selector_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(BackendCandidate {
        name: "low_pri".into(),
        capabilities: vec![Capability::Streaming],
        priority: 10,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "high_pri".into(),
        capabilities: vec![Capability::Streaming],
        priority: 1,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "high_pri");
}

#[test]
fn t86_selector_disabled_candidate_skipped() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "disabled".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: false,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "enabled".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "enabled");
}

#[test]
fn t87_selector_candidate_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    sel.add_candidate(BackendCandidate {
        name: "b".into(),
        capabilities: vec![],
        priority: 0,
        enabled: false,
        metadata: BTreeMap::new(),
    });
    assert_eq!(sel.candidate_count(), 2);
    assert_eq!(sel.enabled_count(), 1);
}

#[test]
fn t88_selector_select_all() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let all = sel.select_all(&[]);
    assert!(all.is_empty());
}

#[test]
fn t89_selector_with_result_unmet() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "a".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let result = sel.select_with_result(&[Capability::McpClient]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn t90_selector_with_result_success() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(BackendCandidate {
        name: "found".into(),
        capabilities: vec![Capability::Streaming],
        priority: 0,
        enabled: true,
        metadata: BTreeMap::new(),
    });
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "found");
    assert!(result.unmet_capabilities.is_empty());
}

// ===========================================================================
// 13. CapabilityMatrix integration (tests 91–97)
// ===========================================================================

#[test]
fn t91_capability_matrix_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.is_empty());
    assert_eq!(m.backend_count(), 0);
}

#[test]
fn t92_capability_matrix_register() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    assert!(m.supports("mock", &Capability::Streaming));
    assert!(m.supports("mock", &Capability::ToolRead));
}

#[test]
fn t93_capability_matrix_not_supported() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    assert!(!m.supports("mock", &Capability::McpClient));
}

#[test]
fn t94_capability_matrix_backends_for() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("c", vec![Capability::ToolRead]);
    let streaming = m.backends_for(&Capability::Streaming);
    assert_eq!(streaming.len(), 2);
}

#[test]
fn t95_capability_matrix_common_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("b", vec![Capability::Streaming, Capability::ToolWrite]);
    let common = m.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
}

#[test]
fn t96_capability_matrix_evaluate() {
    let mut m = CapabilityMatrix::new();
    m.register("half", vec![Capability::Streaming]);
    let report = m.evaluate("half", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.supported.len(), 1);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn t97_capability_matrix_best_backend() {
    let mut m = CapabilityMatrix::new();
    m.register("partial", vec![Capability::Streaming]);
    m.register("full", vec![Capability::Streaming, Capability::ToolRead]);
    let best = m.best_backend(&[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(best.as_deref(), Some("full"));
}

// ===========================================================================
// 14. Registry + trait object dynamic dispatch (tests 98–101)
// ===========================================================================

#[tokio::test]
async fn t98_dynamic_backend_map_dispatch() {
    let mut backends: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    backends.insert("mock".into(), Box::new(MockBackend));
    backends.insert("stub".into(), Box::new(StubBackend::new("stub")));

    let b = backends.get("mock").unwrap();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = b.run(Uuid::new_v4(), wo("dispatch"), tx).await.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t99_dynamic_backend_map_stub_dispatch() {
    let mut backends: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    backends.insert("stub".into(), Box::new(StubBackend::new("stub")));

    let b = backends.get("stub").unwrap();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = b
        .run(Uuid::new_v4(), wo("stub-dispatch"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, "stub");
}

#[tokio::test]
async fn t100_dynamic_select_and_run() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(StubBackend::new("a").with_cap(Capability::Streaming, SupportLevel::Native)),
        Box::new(StubBackend::new("b").with_cap(Capability::ToolRead, SupportLevel::Native)),
    ];

    // Simulate selection: find first that has Streaming.
    let selected = backends
        .iter()
        .find(|b| b.capabilities().contains_key(&Capability::Streaming))
        .unwrap();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = selected
        .run(Uuid::new_v4(), wo("select"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, "a");
}

#[test]
fn t101_arc_backend_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<dyn Backend>>();
}

// ===========================================================================
// 15. Registry metadata edge cases (tests 102–105)
// ===========================================================================

#[test]
fn t102_metadata_with_rate_limit() {
    let mut reg = BackendRegistry::new();
    let mut m = metadata("limited", "openai");
    m.rate_limit = Some(RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    });
    reg.register_with_metadata("limited", m);
    let found = reg.metadata("limited").unwrap();
    let rl = found.rate_limit.as_ref().unwrap();
    assert_eq!(rl.requests_per_minute, 60);
    assert_eq!(rl.tokens_per_minute, 100_000);
    assert_eq!(rl.concurrent_requests, 5);
}

#[test]
fn t103_metadata_max_tokens() {
    let mut reg = BackendRegistry::new();
    let mut m = metadata("big", "openai");
    m.max_tokens = Some(128_000);
    reg.register_with_metadata("big", m);
    assert_eq!(reg.metadata("big").unwrap().max_tokens, Some(128_000));
}

#[test]
fn t104_registry_by_dialect_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.by_dialect("openai").is_empty());
}

#[test]
fn t105_healthy_backends_empty_registry() {
    let reg = BackendRegistry::new();
    assert!(reg.healthy_backends().is_empty());
}
