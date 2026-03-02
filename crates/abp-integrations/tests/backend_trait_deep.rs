// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the Backend trait, MockBackend, trait object safety,
//! event channels, registry patterns, and lifecycle semantics.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionLane, ExecutionMode, MinSupport,
    Outcome, Receipt, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_integrations::{
    Backend, MockBackend, ensure_capability_requirements, extract_execution_mode,
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

fn work_order_with_requirements(reqs: Vec<CapabilityRequirement>) -> WorkOrder {
    WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements { required: reqs })
        .build()
}

/// A minimal custom backend for testing trait object safety and registry patterns.
#[derive(Debug, Clone)]
struct StubBackend {
    name: String,
    caps: CapabilityManifest,
    outcome: Outcome,
}

impl StubBackend {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            caps: CapabilityManifest::new(),
            outcome: Outcome::Complete,
        }
    }

    fn with_capability(mut self, cap: Capability, level: SupportLevel) -> Self {
        self.caps.insert(cap, level);
        self
    }

    #[allow(dead_code)]
    fn with_outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
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
        let started = chrono::Utc::now();
        let ev = AgentEvent {
            ts: started,
            kind: AgentEventKind::RunStarted {
                message: format!("stub: {}", work_order.task),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;

        let _finished = chrono::Utc::now();
        let receipt = abp_core::ReceiptBuilder::new(&self.name)
            .outcome(self.outcome.clone())
            .build()
            .with_hash()?;
        // Patch run_id and work_order_id into the receipt
        let mut receipt = receipt;
        receipt.meta.run_id = run_id;
        receipt.meta.work_order_id = work_order.id;
        Ok(receipt)
    }
}

/// Backend that always errors on run.
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
// 1. MockBackend basic behavior
// ===========================================================================

#[tokio::test]
async fn mock_run_returns_ok() {
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend
        .run(Uuid::new_v4(), simple_work_order("hi"), tx)
        .await;
    assert!(result.is_ok());
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
async fn mock_receipt_has_non_nil_run_id() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(run_id, simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
    assert!(!receipt.meta.run_id.is_nil());
}

#[tokio::test]
async fn mock_receipt_backend_identity_matches() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.backend.id, MockBackend.identity().id);
}

#[tokio::test]
async fn mock_receipt_capabilities_match() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.capabilities.len(), MockBackend.capabilities().len());
}

// ===========================================================================
// 2. MockBackend event streaming
// ===========================================================================

#[tokio::test]
async fn mock_streams_exactly_four_events() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(
        count, 4,
        "mock emits RunStarted + 2 AssistantMessage + RunCompleted"
    );
}

#[tokio::test]
async fn mock_first_event_is_run_started() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let first = rx.try_recv().unwrap();
    assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn mock_last_event_is_run_completed() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn mock_events_have_monotonic_timestamps() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "timestamps must be non-decreasing"
        );
    }
}

#[tokio::test]
async fn mock_trace_and_stream_are_equal_length() {
    let (tx, mut rx) = mpsc::channel(32);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut stream_count = 0;
    while rx.try_recv().is_ok() {
        stream_count += 1;
    }
    assert_eq!(receipt.trace.len(), stream_count);
}

#[tokio::test]
async fn mock_run_started_message_contains_task() {
    let task = "refactor the auth module";
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order(task), tx)
        .await
        .unwrap();
    let first = rx.try_recv().unwrap();
    if let AgentEventKind::RunStarted { message } = &first.kind {
        assert!(message.contains(task));
    } else {
        panic!("expected RunStarted");
    }
}

// ===========================================================================
// 3. Backend trait object safety
// ===========================================================================

#[test]
fn backend_is_object_safe_box() {
    let _: Box<dyn Backend> = Box::new(MockBackend);
}

#[test]
fn backend_is_object_safe_arc() {
    let _: Arc<dyn Backend> = Arc::new(MockBackend);
}

#[tokio::test]
async fn boxed_backend_runs_correctly() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), simple_work_order("boxed"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn arc_backend_runs_correctly() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), simple_work_order("arc"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn dyn_backend_identity_accessible() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(backend.identity().id, "mock");
}

#[tokio::test]
async fn dyn_backend_capabilities_accessible() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    assert!(!backend.capabilities().is_empty());
}

#[tokio::test]
async fn arc_backend_cloneable_across_tasks() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let b1 = Arc::clone(&backend);
    let b2 = Arc::clone(&backend);

    let h1 = tokio::spawn(async move {
        let (tx, _rx) = mpsc::channel(16);
        b1.run(Uuid::new_v4(), simple_work_order("t1"), tx)
            .await
            .unwrap()
    });
    let h2 = tokio::spawn(async move {
        let (tx, _rx) = mpsc::channel(16);
        b2.run(Uuid::new_v4(), simple_work_order("t2"), tx)
            .await
            .unwrap()
    });

    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

// ===========================================================================
// 4. Backend error handling
// ===========================================================================

#[tokio::test]
async fn failing_backend_returns_error() {
    let backend = FailingBackend;
    let (tx, _rx) = mpsc::channel(16);
    let result = backend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn failing_backend_error_message() {
    let backend = FailingBackend;
    let (tx, _rx) = mpsc::channel(16);
    let err = backend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("always errors"));
}

#[tokio::test]
async fn mock_rejects_unsatisfied_native_streaming_requirement() {
    // MockBackend has Streaming as Native, so requesting Native should pass.
    // But requesting something it doesn't have (McpClient) at Native should fail.
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::McpClient,
        min_support: MinSupport::Native,
    }]);
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn mock_accepts_satisfied_emulated_requirement() {
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Emulated,
    }]);
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn mock_rejects_native_for_emulated_capability() {
    // MockBackend has ToolRead as Emulated; requesting Native should fail.
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Native,
    }]);
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn mock_accepts_native_streaming_requirement() {
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    }]);
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn mock_rejects_multiple_unsatisfied_requirements() {
    let wo = work_order_with_requirements(vec![
        CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        },
        CapabilityRequirement {
            capability: Capability::McpServer,
            min_support: MinSupport::Native,
        },
    ]);
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
}

// ===========================================================================
// 5. Multiple backends in a registry
// ===========================================================================

#[tokio::test]
async fn registry_lookup_by_name() {
    let mut registry: HashMap<String, Box<dyn Backend>> = HashMap::new();
    registry.insert("mock".into(), Box::new(MockBackend));
    registry.insert("stub-a".into(), Box::new(StubBackend::new("stub-a")));

    let backend = registry.get("mock").unwrap();
    assert_eq!(backend.identity().id, "mock");

    let backend = registry.get("stub-a").unwrap();
    assert_eq!(backend.identity().id, "stub-a");
}

#[tokio::test]
async fn registry_with_arc_backends() {
    let registry: BTreeMap<String, Arc<dyn Backend>> = BTreeMap::from([
        ("mock".into(), Arc::new(MockBackend) as Arc<dyn Backend>),
        (
            "stub".into(),
            Arc::new(StubBackend::new("stub")) as Arc<dyn Backend>,
        ),
    ]);
    assert_eq!(registry.len(), 2);
    assert!(registry.contains_key("mock"));
    assert!(registry.contains_key("stub"));
}

#[tokio::test]
async fn registry_run_all_backends() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(MockBackend),
        Box::new(StubBackend::new("alpha")),
        Box::new(StubBackend::new("beta")),
    ];

    for b in &backends {
        let (tx, _rx) = mpsc::channel(16);
        let receipt = b
            .run(Uuid::new_v4(), simple_work_order("test"), tx)
            .await
            .unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn registry_select_by_capability() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(StubBackend::new("no-streaming")),
        Box::new(
            StubBackend::new("with-streaming")
                .with_capability(Capability::Streaming, SupportLevel::Native),
        ),
    ];

    let streaming_backends: Vec<&Box<dyn Backend>> = backends
        .iter()
        .filter(|b| b.capabilities().contains_key(&Capability::Streaming))
        .collect();

    assert_eq!(streaming_backends.len(), 1);
    assert_eq!(streaming_backends[0].identity().id, "with-streaming");
}

#[tokio::test]
async fn registry_handles_mixed_outcomes() {
    let mut results = Vec::new();
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(MockBackend),
        Box::new(FailingBackend),
        Box::new(StubBackend::new("ok")),
    ];

    for b in &backends {
        let (tx, _rx) = mpsc::channel(16);
        let result = b.run(Uuid::new_v4(), simple_work_order("x"), tx).await;
        results.push((b.identity().id.clone(), result.is_ok()));
    }

    assert_eq!(results[0], ("mock".into(), true));
    assert_eq!(results[1], ("failing".into(), false));
    assert_eq!(results[2], ("ok".into(), true));
}

// ===========================================================================
// 6. Backend identification (name, capabilities)
// ===========================================================================

#[test]
fn mock_identity_id() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn mock_identity_has_versions() {
    let id = MockBackend.identity();
    assert!(id.backend_version.is_some());
    assert!(id.adapter_version.is_some());
}

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
fn mock_capabilities_include_tool_write() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolWrite));
}

#[test]
fn mock_capabilities_include_tool_edit() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolEdit));
}

#[test]
fn mock_capabilities_include_tool_bash() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolBash));
}

#[test]
fn mock_capabilities_count() {
    let caps = MockBackend.capabilities();
    assert_eq!(caps.len(), 6);
}

#[test]
fn stub_backend_identity_reflects_name() {
    let b = StubBackend::new("my-custom-backend");
    assert_eq!(b.identity().id, "my-custom-backend");
}

#[test]
fn stub_backend_empty_capabilities_by_default() {
    let b = StubBackend::new("empty");
    assert!(b.capabilities().is_empty());
}

#[test]
fn stub_backend_with_added_capability() {
    let b = StubBackend::new("cap").with_capability(Capability::ToolRead, SupportLevel::Native);
    assert!(matches!(
        b.capabilities().get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

// ===========================================================================
// 7. Work order execution lifecycle
// ===========================================================================

#[tokio::test]
async fn lifecycle_run_id_propagates() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(run_id, simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn lifecycle_work_order_id_propagates() {
    let wo = simple_work_order("t");
    let wo_id = wo.id;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn lifecycle_contract_version_in_receipt() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn lifecycle_started_before_finished() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn lifecycle_duration_matches_timestamps() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let computed = (receipt.meta.finished_at - receipt.meta.started_at)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    assert_eq!(receipt.meta.duration_ms, computed);
}

#[tokio::test]
async fn lifecycle_default_mode_is_mapped() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn lifecycle_passthrough_mode_propagates() {
    let mut wo = simple_work_order("t");
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn lifecycle_work_order_with_model_config() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn lifecycle_work_order_with_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(5).build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn lifecycle_work_order_with_workspace_first_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 8. Receipt production from backend
// ===========================================================================

#[tokio::test]
async fn receipt_hash_is_present() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn receipt_hash_is_64_hex_chars() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn receipt_hash_is_deterministic_recompute() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn receipt_trace_non_empty() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_artifacts_empty_for_mock() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
async fn receipt_verification_harness_ok() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn receipt_usage_zero_tokens() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn receipt_serializes_to_json() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains("\"outcome\":\"complete\""));
}

#[tokio::test]
async fn receipt_roundtrips_through_json() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let parsed: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.meta.run_id, receipt.meta.run_id);
    assert_eq!(parsed.outcome, receipt.outcome);
    assert_eq!(parsed.receipt_sha256, receipt.receipt_sha256);
}

// ===========================================================================
// 9. Event channel behavior (sender/receiver)
// ===========================================================================

#[tokio::test]
async fn channel_all_events_received() {
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn channel_capacity_one_with_consumer() {
    let (tx, mut rx) = mpsc::channel(1);
    let consumer = tokio::spawn(async move {
        let mut count = 0u32;
        while rx.recv().await.is_some() {
            count += 1;
        }
        count
    });
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let count = consumer.await.unwrap();
    assert!(count >= 3);
}

#[tokio::test]
async fn channel_large_capacity_no_loss() {
    let (tx, mut rx) = mpsc::channel(1024);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 4);
}

#[tokio::test]
async fn channel_events_are_clones_of_trace() {
    let (tx, mut rx) = mpsc::channel(32);
    let receipt = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await
        .unwrap();
    let mut stream_events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        stream_events.push(ev);
    }
    assert_eq!(stream_events.len(), receipt.trace.len());
    for (s, t) in stream_events.iter().zip(receipt.trace.iter()) {
        assert_eq!(
            std::mem::discriminant(&s.kind),
            std::mem::discriminant(&t.kind)
        );
    }
}

#[tokio::test]
async fn channel_dropped_receiver_does_not_panic() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    // MockBackend ignores send errors (uses `let _ = events_tx.send(...)`)
    let result = MockBackend
        .run(Uuid::new_v4(), simple_work_order("t"), tx)
        .await;
    assert!(result.is_ok());
}

// ===========================================================================
// 10. Backend registration and lookup patterns
// ===========================================================================

/// A simple typed registry for Backend implementations.
struct BackendRegistry {
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl BackendRegistry {
    fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    fn register(&mut self, backend: Arc<dyn Backend>) {
        let name = backend.identity().id.clone();
        self.backends.insert(name, backend);
    }

    fn get(&self, name: &str) -> Option<&Arc<dyn Backend>> {
        self.backends.get(name)
    }

    fn list_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.backends.keys().cloned().collect();
        names.sort();
        names
    }

    fn find_with_capability(&self, cap: &Capability) -> Vec<String> {
        let mut result: Vec<_> = self
            .backends
            .iter()
            .filter(|(_, b)| b.capabilities().contains_key(cap))
            .map(|(name, _)| name.clone())
            .collect();
        result.sort();
        result
    }
}

#[test]
fn registry_register_and_list() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(MockBackend));
    reg.register(Arc::new(StubBackend::new("alpha")));
    reg.register(Arc::new(StubBackend::new("beta")));

    let names = reg.list_names();
    assert_eq!(names, vec!["alpha", "beta", "mock"]);
}

#[test]
fn registry_lookup_existing() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(MockBackend));
    assert!(reg.get("mock").is_some());
}

#[test]
fn registry_lookup_missing() {
    let reg = BackendRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_find_with_capability_streaming() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(MockBackend));
    reg.register(Arc::new(StubBackend::new("no-stream")));
    reg.register(Arc::new(
        StubBackend::new("yes-stream").with_capability(Capability::Streaming, SupportLevel::Native),
    ));

    let mut result = reg.find_with_capability(&Capability::Streaming);
    result.sort();
    assert_eq!(result, vec!["mock", "yes-stream"]);
}

#[test]
fn registry_find_with_capability_none_match() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(StubBackend::new("empty")));
    let result = reg.find_with_capability(&Capability::McpClient);
    assert!(result.is_empty());
}

#[tokio::test]
async fn registry_run_looked_up_backend() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(MockBackend));

    let backend = reg.get("mock").unwrap();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), simple_work_order("registry test"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn registry_overwrite_replaces_backend() {
    let mut reg = BackendRegistry::new();
    reg.register(Arc::new(StubBackend::new("x")));
    reg.register(Arc::new(
        StubBackend::new("x").with_capability(Capability::Streaming, SupportLevel::Native),
    ));
    let backend = reg.get("x").unwrap();
    assert!(backend.capabilities().contains_key(&Capability::Streaming));
}

// ===========================================================================
// Additional ensure_capability_requirements tests
// ===========================================================================

#[test]
fn ensure_caps_satisfied_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_caps_satisfied_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_caps_unsatisfied_native_for_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn ensure_caps_unsatisfied_missing() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn ensure_caps_multiple_all_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

// ===========================================================================
// extract_execution_mode edge cases
// ===========================================================================

#[test]
fn extract_mode_empty_vendor_config() {
    let wo = simple_work_order("t");
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_non_object_abp_key() {
    let mut wo = simple_work_order("t");
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!("not_an_object"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_abp_object_without_mode_key() {
    let mut wo = simple_work_order("t");
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!({"other": "value"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_dotted_passthrough() {
    let mut wo = simple_work_order("t");
    wo.config
        .vendor
        .insert("abp.mode".into(), serde_json::json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_nested_takes_priority() {
    let mut wo = simple_work_order("t");
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    wo.config
        .vendor
        .insert("abp.mode".into(), serde_json::json!("mapped"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}
