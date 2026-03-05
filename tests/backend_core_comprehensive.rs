#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for abp-backend-core and abp-backend-mock crates.

use std::sync::Arc;

use abp_backend_core::{
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
    Backend, BackendHealth, BackendMetadata, BackendRegistry, HealthStatus, RateLimit,
};
use abp_backend_mock::scenarios::{
    MockBackendRecorder, MockScenario, RecordedCall, ScenarioMockBackend,
};
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    SupportLevel, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn work_order_with_mode(task: &str, mode: &str) -> WorkOrder {
    let mut wo = WorkOrderBuilder::new(task).build();
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": mode}));
    wo
}

fn work_order_with_requirements(reqs: Vec<CapabilityRequirement>) -> WorkOrder {
    WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements { required: reqs })
        .build()
}

async fn collect_events(mut rx: mpsc::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    events
}

/// A custom backend for testing the trait interface.
#[derive(Debug, Clone)]
struct StubBackend {
    id: String,
    caps: CapabilityManifest,
}

impl StubBackend {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            caps: CapabilityManifest::default(),
        }
    }

    fn with_capability(mut self, cap: Capability, level: SupportLevel) -> Self {
        self.caps.insert(cap, level);
        self
    }
}

#[async_trait]
impl Backend for StubBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
            backend_version: Some("test".to_string()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.caps.clone()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("stub {} responding", self.id),
                },
                ext: None,
            })
            .await;
        anyhow::bail!("stub backend does not produce real receipts")
    }
}

// ===========================================================================
// Module: Backend Trait Interface
// ===========================================================================

#[tokio::test]
async fn trait_mock_identity_id() {
    let b = MockBackend;
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn trait_mock_identity_backend_version() {
    let b = MockBackend;
    assert_eq!(b.identity().backend_version, Some("0.1".to_string()));
}

#[tokio::test]
async fn trait_mock_identity_adapter_version() {
    let b = MockBackend;
    assert_eq!(b.identity().adapter_version, Some("0.1".to_string()));
}

#[tokio::test]
async fn trait_stub_identity_id() {
    let b = StubBackend::new("my-stub");
    assert_eq!(b.identity().id, "my-stub");
}

#[tokio::test]
async fn trait_stub_identity_adapter_is_none() {
    let b = StubBackend::new("x");
    assert!(b.identity().adapter_version.is_none());
}

#[tokio::test]
async fn trait_capabilities_returns_btreemap() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(!caps.is_empty());
}

#[tokio::test]
async fn trait_dyn_dispatch_identity() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn trait_dyn_dispatch_capabilities() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    assert!(b.capabilities().contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn trait_arc_dyn_identity() {
    let b: Arc<dyn Backend> = Arc::new(MockBackend);
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn trait_arc_dyn_run() {
    let b: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("arc test");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    drop(rx);
}

// ===========================================================================
// Module: MockBackend Creation & Configuration
// ===========================================================================

#[test]
fn mock_backend_is_clone() {
    let b = MockBackend;
    let _b2 = b.clone();
}

#[test]
fn mock_backend_is_debug() {
    let b = MockBackend;
    let s = format!("{:?}", b);
    assert!(s.contains("MockBackend"));
}

#[test]
fn mock_backend_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MockBackend>();
}

#[test]
fn mock_backend_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<MockBackend>();
}

#[test]
fn mock_backend_identity_stable() {
    let b1 = MockBackend;
    let b2 = MockBackend;
    assert_eq!(b1.identity().id, b2.identity().id);
}

// ===========================================================================
// Module: MockBackend Event Streaming
// ===========================================================================

#[tokio::test]
async fn mock_run_streams_events() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("hello");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    assert!(events.len() >= 3, "expected at least 3 events");
}

#[tokio::test]
async fn mock_run_first_event_is_run_started() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("test");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted"
    );
}

#[tokio::test]
async fn mock_run_last_event_is_run_completed() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("test");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let last = events.last().unwrap();
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last event should be RunCompleted"
    );
}

#[tokio::test]
async fn mock_run_contains_assistant_message() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("test");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let has_msg = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    assert!(has_msg, "expected at least one AssistantMessage event");
}

#[tokio::test]
async fn mock_run_events_have_timestamps() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("ts test");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    for ev in &events {
        assert!(ev.ts <= Utc::now());
    }
}

#[tokio::test]
async fn mock_run_events_ext_is_none() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("ext");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn mock_run_exactly_four_events() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("count");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn mock_run_started_message_contains_task() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("my-unique-task");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my-unique-task"));
    } else {
        panic!("first event not RunStarted");
    }
}

// ===========================================================================
// Module: MockBackend Receipt Generation
// ===========================================================================

#[tokio::test]
async fn mock_receipt_outcome_complete() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("receipt test");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_receipt_has_hash() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("hash test");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn mock_receipt_hash_is_hex() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("hex");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let hash = receipt.receipt_sha256.unwrap();
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn mock_receipt_contract_version() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("ver");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_receipt_run_id_matches() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let run_id = Uuid::new_v4();
    let wo = simple_work_order("id test");
    let receipt = b.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn mock_receipt_work_order_id_matches() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("wo id");
    let wo_id = wo.id;
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn mock_receipt_backend_identity() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("identity");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn mock_receipt_has_trace() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("trace");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn mock_receipt_trace_matches_events() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("trace eq");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn mock_receipt_usage_tokens_zero() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("usage");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn mock_receipt_estimated_cost_zero() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("cost");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn mock_receipt_verification_harness_ok() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("verification");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn mock_receipt_no_artifacts() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("artifacts");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
async fn mock_receipt_started_before_finished() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("timing");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn mock_receipt_mode_default_mapped() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("mode");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mock_receipt_mode_passthrough() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = work_order_with_mode("passthrough", "passthrough");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn mock_receipt_unique_hashes() {
    let b = MockBackend;
    let (tx1, _rx1) = mpsc::channel(32);
    let wo1 = simple_work_order("hash1");
    let r1 = b.run(Uuid::new_v4(), wo1, tx1).await.unwrap();

    let (tx2, _rx2) = mpsc::channel(32);
    let wo2 = simple_work_order("hash2");
    let r2 = b.run(Uuid::new_v4(), wo2, tx2).await.unwrap();

    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// Module: Backend Capability Reporting
// ===========================================================================

#[test]
fn mock_caps_include_streaming() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn mock_caps_include_tool_read() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn mock_caps_include_tool_write() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn mock_caps_include_tool_edit() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn mock_caps_include_tool_bash() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn mock_caps_include_structured_output() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn mock_caps_count() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert_eq!(caps.len(), 6);
}

#[test]
fn mock_caps_no_vision() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(!caps.contains_key(&Capability::Vision));
}

#[test]
fn mock_caps_no_audio() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(!caps.contains_key(&Capability::Audio));
}

#[test]
fn stub_caps_empty_by_default() {
    let b = StubBackend::new("x");
    assert!(b.capabilities().is_empty());
}

#[test]
fn stub_caps_with_single_cap() {
    let b = StubBackend::new("x").with_capability(Capability::Vision, SupportLevel::Native);
    assert!(matches!(
        b.capabilities().get(&Capability::Vision),
        Some(SupportLevel::Native)
    ));
}

// ===========================================================================
// Module: ensure_capability_requirements
// ===========================================================================

#[test]
fn ensure_empty_requirements_ok() {
    let reqs = CapabilityRequirements { required: vec![] };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_satisfied_native_streaming() {
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
fn ensure_satisfied_emulated_tool_read() {
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
fn ensure_unsatisfied_native_tool_read() {
    // ToolRead is Emulated in MockBackend, so Native should fail
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
fn ensure_unsatisfied_missing_capability() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsatisfied"));
}

#[test]
fn ensure_multiple_requirements_all_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_multiple_requirements_one_unsatisfied() {
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
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn ensure_against_empty_manifest() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = CapabilityManifest::default();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn ensure_empty_reqs_against_empty_manifest() {
    let reqs = CapabilityRequirements { required: vec![] };
    let caps = CapabilityManifest::default();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

// ===========================================================================
// Module: extract_execution_mode
// ===========================================================================

#[test]
fn extract_mode_default_is_mapped() {
    let wo = simple_work_order("test");
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_passthrough_nested() {
    let wo = work_order_with_mode("test", "passthrough");
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_mapped_explicit() {
    let wo = work_order_with_mode("test", "mapped");
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_flat_key() {
    let mut wo = simple_work_order("test");
    wo.config
        .vendor
        .insert("abp.mode".to_string(), json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_mode_invalid_value_defaults_mapped() {
    let mut wo = simple_work_order("test");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "invalid_mode"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_mode_nested_takes_precedence() {
    let mut wo = simple_work_order("test");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    wo.config
        .vendor
        .insert("abp.mode".to_string(), json!("mapped"));
    // nested should take precedence
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

// ===========================================================================
// Module: validate_passthrough_compatibility
// ===========================================================================

#[test]
fn passthrough_validation_ok() {
    let wo = simple_work_order("test");
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

#[test]
fn passthrough_validation_with_requirements() {
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    }]);
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

// ===========================================================================
// Module: BackendRegistry
// ===========================================================================

fn sample_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "1.0".to_string(),
        max_tokens: None,
        supports_streaming: true,
        supports_tools: false,
        rate_limit: None,
    }
}

#[test]
fn registry_new_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn registry_register_one() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("mock"));
}

#[test]
fn registry_register_multiple() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "anthropic"));
    reg.register_with_metadata("c", sample_metadata("c", "openai"));
    assert_eq!(reg.len(), 3);
}

#[test]
fn registry_contains_registered() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    assert!(reg.contains("mock"));
}

#[test]
fn registry_not_contains_unregistered() {
    let reg = BackendRegistry::new();
    assert!(!reg.contains("nonexistent"));
}

#[test]
fn registry_metadata_lookup() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    let meta = reg.metadata("mock").unwrap();
    assert_eq!(meta.name, "mock");
    assert_eq!(meta.dialect, "openai");
}

#[test]
fn registry_metadata_missing() {
    let reg = BackendRegistry::new();
    assert!(reg.metadata("foo").is_none());
}

#[test]
fn registry_list_sorted() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("charlie", sample_metadata("charlie", "x"));
    reg.register_with_metadata("alpha", sample_metadata("alpha", "x"));
    reg.register_with_metadata("bravo", sample_metadata("bravo", "x"));
    assert_eq!(reg.list(), vec!["alpha", "bravo", "charlie"]);
}

#[test]
fn registry_list_empty() {
    let reg = BackendRegistry::new();
    let list: Vec<&str> = reg.list();
    assert!(list.is_empty());
}

#[test]
fn registry_remove_existing() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    let removed = reg.remove("mock");
    assert!(removed.is_some());
    assert!(!reg.contains("mock"));
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = BackendRegistry::new();
    let removed = reg.remove("foo");
    assert!(removed.is_none());
}

#[test]
fn registry_by_dialect() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "anthropic"));
    reg.register_with_metadata("c", sample_metadata("c", "openai"));
    let openai = reg.by_dialect("openai");
    assert_eq!(openai, vec!["a", "c"]);
}

#[test]
fn registry_by_dialect_none() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    assert!(reg.by_dialect("anthropic").is_empty());
}

#[test]
fn registry_replace_metadata() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    reg.register_with_metadata("mock", sample_metadata("mock", "anthropic"));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.metadata("mock").unwrap().dialect, "anthropic");
}

// ===========================================================================
// Module: BackendHealth
// ===========================================================================

#[test]
fn health_default_status_unknown() {
    let h = BackendHealth::default();
    assert_eq!(h.status, HealthStatus::Unknown);
}

#[test]
fn health_default_no_last_check() {
    let h = BackendHealth::default();
    assert!(h.last_check.is_none());
}

#[test]
fn health_default_no_latency() {
    let h = BackendHealth::default();
    assert!(h.latency_ms.is_none());
}

#[test]
fn health_default_error_rate_zero() {
    let h = BackendHealth::default();
    assert_eq!(h.error_rate, 0.0);
}

#[test]
fn health_default_no_consecutive_failures() {
    let h = BackendHealth::default();
    assert_eq!(h.consecutive_failures, 0);
}

#[test]
fn health_status_healthy() {
    let h = BackendHealth {
        status: HealthStatus::Healthy,
        ..Default::default()
    };
    assert_eq!(h.status, HealthStatus::Healthy);
}

#[test]
fn health_status_degraded() {
    let h = BackendHealth {
        status: HealthStatus::Degraded,
        ..Default::default()
    };
    assert_eq!(h.status, HealthStatus::Degraded);
}

#[test]
fn health_status_unhealthy() {
    let h = BackendHealth {
        status: HealthStatus::Unhealthy,
        ..Default::default()
    };
    assert_eq!(h.status, HealthStatus::Unhealthy);
}

#[test]
fn health_is_clone() {
    let h = BackendHealth::default();
    let h2 = h.clone();
    assert_eq!(h2.status, HealthStatus::Unknown);
}

#[test]
fn health_serde_roundtrip() {
    let h = BackendHealth {
        status: HealthStatus::Healthy,
        last_check: Some(Utc::now()),
        latency_ms: Some(42),
        error_rate: 0.05,
        consecutive_failures: 2,
    };
    let json = serde_json::to_string(&h).unwrap();
    let h2: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(h2.status, HealthStatus::Healthy);
    assert_eq!(h2.latency_ms, Some(42));
    assert_eq!(h2.consecutive_failures, 2);
}

#[test]
fn health_status_serde_snake_case() {
    let json = serde_json::to_string(&HealthStatus::Healthy).unwrap();
    assert_eq!(json, "\"healthy\"");
    let json = serde_json::to_string(&HealthStatus::Degraded).unwrap();
    assert_eq!(json, "\"degraded\"");
    let json = serde_json::to_string(&HealthStatus::Unhealthy).unwrap();
    assert_eq!(json, "\"unhealthy\"");
    let json = serde_json::to_string(&HealthStatus::Unknown).unwrap();
    assert_eq!(json, "\"unknown\"");
}

// ===========================================================================
// Module: Registry Health Tracking
// ===========================================================================

#[test]
fn registry_health_default_on_register() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    let h = reg.health("mock").unwrap();
    assert_eq!(h.status, HealthStatus::Unknown);
}

#[test]
fn registry_health_missing() {
    let reg = BackendRegistry::new();
    assert!(reg.health("nonexistent").is_none());
}

#[test]
fn registry_update_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "openai"));
    reg.update_health(
        "mock",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..Default::default()
        },
    );
    assert_eq!(reg.health("mock").unwrap().status, HealthStatus::Healthy);
}

#[test]
fn registry_healthy_backends() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "x"));
    reg.register_with_metadata("b", sample_metadata("b", "x"));
    reg.register_with_metadata("c", sample_metadata("c", "x"));
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..Default::default()
        },
    );
    reg.update_health(
        "c",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..Default::default()
        },
    );
    assert_eq!(reg.healthy_backends(), vec!["a", "c"]);
}

#[test]
fn registry_healthy_backends_none() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "x"));
    assert!(reg.healthy_backends().is_empty());
}

#[test]
fn registry_remove_clears_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("mock", sample_metadata("mock", "x"));
    reg.update_health(
        "mock",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..Default::default()
        },
    );
    reg.remove("mock");
    assert!(reg.health("mock").is_none());
}

// ===========================================================================
// Module: RateLimit and BackendMetadata
// ===========================================================================

#[test]
fn rate_limit_serde_roundtrip() {
    let rl = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    let json = serde_json::to_string(&rl).unwrap();
    let rl2: RateLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(rl, rl2);
}

#[test]
fn metadata_serde_roundtrip() {
    let m = BackendMetadata {
        name: "test".into(),
        dialect: "openai".into(),
        version: "2.0".into(),
        max_tokens: Some(128000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: Some(RateLimit {
            requests_per_minute: 100,
            tokens_per_minute: 500_000,
            concurrent_requests: 10,
        }),
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.name, "test");
    assert_eq!(m2.max_tokens, Some(128000));
    assert!(m2.supports_streaming);
    assert!(m2.supports_tools);
    assert!(m2.rate_limit.is_some());
}

#[test]
fn metadata_no_rate_limit() {
    let m = sample_metadata("simple", "openai");
    assert!(m.rate_limit.is_none());
}

#[test]
fn metadata_clone() {
    let m = sample_metadata("x", "openai");
    let m2 = m.clone();
    assert_eq!(m2.name, "x");
}

// ===========================================================================
// Module: ScenarioMockBackend
// ===========================================================================

#[tokio::test]
async fn scenario_success_basic() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "hello".to_string(),
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("scenario");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let events = collect_events(rx).await;
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn scenario_success_contains_text() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "custom-response".to_string(),
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("scenario");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let has_text = events.iter().any(|e| match &e.kind {
        AgentEventKind::AssistantMessage { text } => text.contains("custom-response"),
        _ => false,
    });
    assert!(has_text);
}

#[tokio::test]
async fn scenario_identity() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.identity().id, "scenario-mock");
}

#[tokio::test]
async fn scenario_call_count_starts_zero() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.call_count(), 0);
}

#[tokio::test]
async fn scenario_call_count_increments() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("a"), tx).await;
    assert_eq!(b.call_count(), 1);
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("b"), tx).await;
    assert_eq!(b.call_count(), 2);
}

#[tokio::test]
async fn scenario_streaming_emits_deltas() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into(), "c".into()],
        chunk_delay_ms: 0,
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("stream");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 3);
}

#[tokio::test]
async fn scenario_streaming_delta_content() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["first".into(), "second".into()],
        chunk_delay_ms: 0,
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("stream");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["first", "second"]);
}

#[tokio::test]
async fn scenario_permanent_error() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "ABP-B001".into(),
        message: "gone forever".into(),
    });
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("err");
    let err = b.run(Uuid::new_v4(), wo, tx).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ABP-B001"));
    assert!(msg.contains("gone forever"));
}

#[tokio::test]
async fn scenario_permanent_error_records_call() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E1".into(),
        message: "bad".into(),
    });
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("err"), tx).await;
    let calls = b.calls().await;
    assert_eq!(calls.len(), 1);
    assert!(calls[0].result.is_err());
}

#[tokio::test]
async fn scenario_permanent_error_last_error() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E1".into(),
        message: "boom".into(),
    });
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("err"), tx).await;
    let last = b.last_error().await;
    assert!(last.is_some());
    assert!(last.unwrap().contains("boom"));
}

#[tokio::test]
async fn scenario_transient_then_success() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "recovered".into(),
        }),
    });

    // first two calls should fail
    let (tx, _rx) = mpsc::channel(32);
    assert!(b
        .run(Uuid::new_v4(), simple_work_order("t1"), tx)
        .await
        .is_err());
    let (tx, _rx) = mpsc::channel(32);
    assert!(b
        .run(Uuid::new_v4(), simple_work_order("t2"), tx)
        .await
        .is_err());

    // third call should succeed
    let (tx, _rx) = mpsc::channel(32);
    let receipt = b
        .run(Uuid::new_v4(), simple_work_order("t3"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(b.call_count(), 3);
}

#[tokio::test]
async fn scenario_timeout() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 10 });
    let (tx, _rx) = mpsc::channel(32);
    let err = b
        .run(Uuid::new_v4(), simple_work_order("timeout"), tx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn scenario_rate_limited() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 1000,
    });
    let (tx, _rx) = mpsc::channel(32);
    let err = b
        .run(Uuid::new_v4(), simple_work_order("rl"), tx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("rate limited"));
}

#[tokio::test]
async fn scenario_clone_shares_state() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let b2 = b.clone();
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("a"), tx).await;
    // clone should see the recorded call
    let calls = b2.calls().await;
    assert_eq!(calls.len(), 1);
}

#[tokio::test]
async fn scenario_last_call() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.last_call().await.is_none());
    let (tx, _rx) = mpsc::channel(32);
    let _ = b.run(Uuid::new_v4(), simple_work_order("first"), tx).await;
    let last = b.last_call().await.unwrap();
    assert_eq!(last.work_order.task, "first");
}

// ===========================================================================
// Module: MockBackendRecorder
// ===========================================================================

#[tokio::test]
async fn recorder_wraps_mock() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert_eq!(rec.identity().id, "mock");
}

#[tokio::test]
async fn recorder_capabilities_pass_through() {
    let rec = MockBackendRecorder::new(MockBackend);
    let caps = rec.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn recorder_records_calls() {
    let rec = MockBackendRecorder::new(MockBackend);
    let (tx, _rx) = mpsc::channel(32);
    let _ = rec
        .run(Uuid::new_v4(), simple_work_order("rec"), tx)
        .await
        .unwrap();
    assert_eq!(rec.call_count().await, 1);
}

#[tokio::test]
async fn recorder_call_count_zero_initially() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert_eq!(rec.call_count().await, 0);
}

#[tokio::test]
async fn recorder_last_call_none_initially() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert!(rec.last_call().await.is_none());
}

#[tokio::test]
async fn recorder_last_call_populated() {
    let rec = MockBackendRecorder::new(MockBackend);
    let (tx, _rx) = mpsc::channel(32);
    let _ = rec
        .run(Uuid::new_v4(), simple_work_order("my-task"), tx)
        .await;
    let last = rec.last_call().await.unwrap();
    assert_eq!(last.work_order.task, "my-task");
    assert!(last.result.is_ok());
}

#[tokio::test]
async fn recorder_multiple_calls() {
    let rec = MockBackendRecorder::new(MockBackend);
    for i in 0..3 {
        let (tx, _rx) = mpsc::channel(32);
        let _ = rec
            .run(Uuid::new_v4(), simple_work_order(&format!("task-{i}")), tx)
            .await;
    }
    assert_eq!(rec.call_count().await, 3);
    let calls = rec.calls().await;
    assert_eq!(calls[0].work_order.task, "task-0");
    assert_eq!(calls[2].work_order.task, "task-2");
}

#[tokio::test]
async fn recorder_clone_shares_calls() {
    let rec = MockBackendRecorder::new(MockBackend);
    let rec2 = rec.clone();
    let (tx, _rx) = mpsc::channel(32);
    let _ = rec
        .run(Uuid::new_v4(), simple_work_order("shared"), tx)
        .await;
    assert_eq!(rec2.call_count().await, 1);
}

// ===========================================================================
// Module: Error Handling in Backends
// ===========================================================================

#[tokio::test]
async fn mock_run_unsatisfied_requirements_error() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::Vision,
        min_support: MinSupport::Native,
    }]);
    let err = b.run(Uuid::new_v4(), wo, tx).await.unwrap_err();
    assert!(err.to_string().contains("capability requirements"));
}

#[tokio::test]
async fn scenario_unsatisfied_requirements_error() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (tx, _rx) = mpsc::channel(32);
    let wo = work_order_with_requirements(vec![CapabilityRequirement {
        capability: Capability::Vision,
        min_support: MinSupport::Native,
    }]);
    assert!(b.run(Uuid::new_v4(), wo, tx).await.is_err());
}

#[tokio::test]
async fn stub_backend_errors() {
    let b = StubBackend::new("test");
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("stub fail");
    assert!(b.run(Uuid::new_v4(), wo, tx).await.is_err());
}

// ===========================================================================
// Module: Async Streaming via mpsc
// ===========================================================================

#[tokio::test]
async fn channel_receives_all_events() {
    let b = MockBackend;
    let (tx, mut rx) = mpsc::channel(1); // small buffer to test backpressure
    let wo = simple_work_order("channel");
    let handle = tokio::spawn(async move { b.run(Uuid::new_v4(), wo, tx).await });
    let mut count = 0;
    while let Some(_) = rx.recv().await {
        count += 1;
    }
    assert_eq!(count, 4);
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn channel_drop_receiver_doesnt_panic() {
    let b = MockBackend;
    let (tx, rx) = mpsc::channel(32);
    drop(rx); // drop receiver before run
              // The mock backend uses `let _ = tx.send(...)` so it should not panic
    let result = b.run(Uuid::new_v4(), simple_work_order("drop"), tx).await;
    // It may or may not succeed, but should not panic
    let _ = result;
}

#[tokio::test]
async fn concurrent_runs_independent_channels() {
    let b = Arc::new(MockBackend);
    let mut handles = vec![];
    for i in 0..5 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, rx) = mpsc::channel(32);
            let wo = simple_work_order(&format!("concurrent-{i}"));
            let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
            let events = collect_events(rx).await;
            (receipt, events)
        }));
    }
    for h in handles {
        let (receipt, events) = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(events.len(), 4);
    }
}

#[tokio::test]
async fn streaming_scenario_events_arrive_in_order() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["1".into(), "2".into(), "3".into(), "4".into(), "5".into()],
        chunk_delay_ms: 0,
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("order");
    let _ = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let events = collect_events(rx).await;
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["1", "2", "3", "4", "5"]);
}

// ===========================================================================
// Module: MockScenario Serde
// ===========================================================================

#[test]
fn scenario_success_serde() {
    let s = MockScenario::Success {
        delay_ms: 100,
        text: "hello".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::Success { delay_ms, text } => {
            assert_eq!(delay_ms, 100);
            assert_eq!(text, "hello");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn scenario_streaming_serde() {
    let s = MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 50,
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::StreamingSuccess {
            chunks,
            chunk_delay_ms,
        } => {
            assert_eq!(chunks, vec!["a", "b"]);
            assert_eq!(chunk_delay_ms, 50);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn scenario_permanent_error_serde() {
    let s = MockScenario::PermanentError {
        code: "ERR".into(),
        message: "oops".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::PermanentError { code, message } => {
            assert_eq!(code, "ERR");
            assert_eq!(message, "oops");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn scenario_timeout_serde() {
    let s = MockScenario::Timeout { after_ms: 5000 };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::Timeout { after_ms } => assert_eq!(after_ms, 5000),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn scenario_rate_limited_serde() {
    let s = MockScenario::RateLimited {
        retry_after_ms: 2000,
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 2000),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn scenario_transient_serde() {
    let s = MockScenario::TransientError {
        fail_count: 3,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MockScenario = serde_json::from_str(&json).unwrap();
    match s2 {
        MockScenario::TransientError { fail_count, then } => {
            assert_eq!(fail_count, 3);
            assert!(matches!(*then, MockScenario::Success { .. }));
        }
        _ => panic!("wrong variant"),
    }
}

// ===========================================================================
// Module: RecordedCall
// ===========================================================================

#[test]
fn recorded_call_serde_success() {
    let rc = RecordedCall {
        work_order: simple_work_order("test"),
        timestamp: Utc::now(),
        duration_ms: 42,
        result: Ok(Outcome::Complete),
    };
    let json = serde_json::to_string(&rc).unwrap();
    let rc2: RecordedCall = serde_json::from_str(&json).unwrap();
    assert_eq!(rc2.duration_ms, 42);
    assert!(rc2.result.is_ok());
}

#[test]
fn recorded_call_serde_error() {
    let rc = RecordedCall {
        work_order: simple_work_order("test"),
        timestamp: Utc::now(),
        duration_ms: 10,
        result: Err("something broke".into()),
    };
    let json = serde_json::to_string(&rc).unwrap();
    let rc2: RecordedCall = serde_json::from_str(&json).unwrap();
    assert!(rc2.result.is_err());
    assert_eq!(rc2.result.unwrap_err(), "something broke");
}

#[test]
fn recorded_call_clone() {
    let rc = RecordedCall {
        work_order: simple_work_order("test"),
        timestamp: Utc::now(),
        duration_ms: 1,
        result: Ok(Outcome::Complete),
    };
    let rc2 = rc.clone();
    assert_eq!(rc2.duration_ms, 1);
}

// ===========================================================================
// Module: BackendIdentity
// ===========================================================================

#[test]
fn backend_identity_fields() {
    let id = BackendIdentity {
        id: "my-backend".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("2.0".into()),
    };
    assert_eq!(id.id, "my-backend");
    assert_eq!(id.backend_version.as_deref(), Some("1.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("2.0"));
}

#[test]
fn backend_identity_optional_versions() {
    let id = BackendIdentity {
        id: "x".into(),
        backend_version: None,
        adapter_version: None,
    };
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
}

#[test]
fn backend_identity_clone() {
    let id = BackendIdentity {
        id: "test".into(),
        backend_version: Some("v1".into()),
        adapter_version: None,
    };
    let id2 = id.clone();
    assert_eq!(id2.id, "test");
}

// ===========================================================================
// Module: SupportLevel satisfaction logic
// ===========================================================================

#[test]
fn native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_does_not_satisfy_native() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_does_not_satisfy_emulated() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_satisfies_emulated() {
    let level = SupportLevel::Restricted {
        reason: "quota".into(),
    };
    assert!(level.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_does_not_satisfy_native() {
    let level = SupportLevel::Restricted {
        reason: "quota".into(),
    };
    assert!(!level.satisfies(&MinSupport::Native));
}

// ===========================================================================
// Module: Registry Clone and Debug
// ===========================================================================

#[test]
fn registry_clone() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    let reg2 = reg.clone();
    assert_eq!(reg2.len(), 1);
    assert!(reg2.contains("a"));
}

#[test]
fn registry_debug() {
    let reg = BackendRegistry::new();
    let s = format!("{:?}", reg);
    assert!(s.contains("BackendRegistry"));
}

// ===========================================================================
// Module: Edge cases
// ===========================================================================

#[test]
fn registry_register_empty_name() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("", sample_metadata("", "x"));
    assert!(reg.contains(""));
    assert_eq!(reg.len(), 1);
}

#[tokio::test]
async fn mock_run_empty_task() {
    let b = MockBackend;
    let (tx, _rx) = mpsc::channel(32);
    let wo = simple_work_order("");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_streaming_empty_chunks() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec![],
        chunk_delay_ms: 0,
    });
    let (tx, rx) = mpsc::channel(32);
    let wo = simple_work_order("empty");
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let events = collect_events(rx).await;
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 0);
}

#[tokio::test]
async fn scenario_transient_zero_fails_succeeds_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 0,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let (tx, _rx) = mpsc::channel(32);
    let receipt = b
        .run(Uuid::new_v4(), simple_work_order("z"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn registry_metadata_supports_streaming_flag() {
    let m = BackendMetadata {
        name: "fast".into(),
        dialect: "openai".into(),
        version: "1.0".into(),
        max_tokens: Some(4096),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    };
    assert!(m.supports_streaming);
    assert!(m.supports_tools);
    assert_eq!(m.max_tokens, Some(4096));
}

#[tokio::test]
async fn recorder_wrapping_scenario() {
    let scenario = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "recorded".into(),
    });
    let rec = MockBackendRecorder::new(scenario);
    let (tx, _rx) = mpsc::channel(32);
    let receipt = rec
        .run(Uuid::new_v4(), simple_work_order("rec"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(rec.call_count().await, 1);
    assert_eq!(rec.identity().id, "scenario-mock");
}
