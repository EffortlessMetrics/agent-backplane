// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the Backend trait, registry, capabilities, metrics,
//! selection, health, configuration, streaming, concurrency, and timeouts.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use abp_backend_core::health::{BackendHealth, HealthStatus};
use abp_backend_core::metadata::{BackendMetadata, RateLimit};
use abp_backend_core::registry::BackendRegistry;
use abp_backend_core::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrderBuilder,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Test helpers
// ═══════════════════════════════════════════════════════════════════════

fn make_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_work_order_with_requirements(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test").requirements(reqs).build()
}

fn sample_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "1.0.0".to_string(),
        max_tokens: Some(4096),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

fn make_receipt(run_id: Uuid, wo: &abp_core::WorkOrder, backend_id: &str) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend_id.to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
    .with_hash()
    .unwrap()
}

/// Run a backend and collect both the receipt and all streamed events.
async fn run_and_collect(
    backend: &dyn Backend,
    work_order: abp_core::WorkOrder,
) -> (Receipt, Vec<AgentEvent>) {
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(run_id, work_order, tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

// ═══════════════════════════════════════════════════════════════════════
// Custom backend implementations for testing
// ═══════════════════════════════════════════════════════════════════════

/// A backend that always errors.
#[derive(Debug, Clone)]
struct FailingBackend;

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".to_string(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: abp_core::WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        anyhow::bail!("intentional backend failure")
    }
}

/// A backend that streams a configurable number of events before returning.
#[derive(Debug, Clone)]
struct StreamingBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for StreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "streaming".to_string(),
            backend_version: Some("1.0".to_string()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: abp_core::WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let started = Utc::now();
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "streaming start".into(),
                },
                ext: None,
            })
            .await;
        for i in 0..self.event_count {
            let _ = events_tx
                .send(AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("chunk-{i}"),
                    },
                    ext: None,
                })
                .await;
        }
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .await;
        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// A backend that tracks how many times `run` is invoked.
#[derive(Debug)]
struct CountingBackend {
    count: AtomicU32,
}

impl CountingBackend {
    fn new() -> Self {
        Self {
            count: AtomicU32::new(0),
        }
    }
    fn run_count(&self) -> u32 {
        self.count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".to_string(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: abp_core::WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        self.count.fetch_add(1, Ordering::SeqCst);
        Ok(make_receipt(run_id, &work_order, "counting"))
    }
}

/// A slow backend that delays for a configurable duration.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay: Duration,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".to_string(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: abp_core::WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        tokio::time::sleep(self.delay).await;
        Ok(make_receipt(run_id, &work_order, "slow"))
    }
}

/// A backend with rich capabilities.
#[derive(Debug, Clone)]
struct RichBackend;

#[async_trait]
impl Backend for RichBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "rich".to_string(),
            backend_version: Some("2.0".to_string()),
            adapter_version: Some("1.0".to_string()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
        m.insert(Capability::ToolWrite, SupportLevel::Native);
        m.insert(Capability::ToolEdit, SupportLevel::Native);
        m.insert(Capability::ToolBash, SupportLevel::Native);
        m.insert(Capability::StructuredOutputJsonSchema, SupportLevel::Native);
        m.insert(Capability::ExtendedThinking, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: abp_core::WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let started = Utc::now();
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "rich start".into(),
                },
                ext: None,
            })
            .await;
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "rich done".into(),
                },
                ext: None,
            })
            .await;
        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Trait compliance — verify custom backends implement Backend
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn trait_compliance_failing_backend_identity() {
    let b = FailingBackend;
    assert_eq!(b.identity().id, "failing");
}

#[test]
fn trait_compliance_failing_backend_capabilities_empty() {
    let b = FailingBackend;
    assert!(b.capabilities().is_empty());
}

#[test]
fn trait_compliance_streaming_backend_identity() {
    let b = StreamingBackend { event_count: 0 };
    assert_eq!(b.identity().id, "streaming");
}

#[test]
fn trait_compliance_streaming_backend_has_streaming_cap() {
    let b = StreamingBackend { event_count: 0 };
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn trait_compliance_rich_backend_identity() {
    let b = RichBackend;
    let id = b.identity();
    assert_eq!(id.id, "rich");
    assert_eq!(id.backend_version.as_deref(), Some("2.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("1.0"));
}

#[test]
fn trait_compliance_rich_backend_capabilities() {
    let b = RichBackend;
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
    assert!(caps.contains_key(&Capability::ToolRead));
    assert!(caps.contains_key(&Capability::ExtendedThinking));
}

#[test]
fn trait_compliance_counting_backend_identity() {
    let b = CountingBackend::new();
    assert_eq!(b.identity().id, "counting");
}

#[test]
fn trait_compliance_object_safety() {
    // Backend must be object-safe (usable as dyn Backend).
    let b: Box<dyn Backend> = Box::new(FailingBackend);
    assert_eq!(b.identity().id, "failing");
}

#[test]
fn trait_compliance_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FailingBackend>();
    assert_send_sync::<StreamingBackend>();
    assert_send_sync::<RichBackend>();
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Run lifecycle — start → stream events → complete
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_lifecycle_streaming_backend_returns_receipt() {
    let b = StreamingBackend { event_count: 3 };
    let wo = make_work_order("lifecycle test");
    let (receipt, _events) = run_and_collect(&b, wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_lifecycle_events_start_and_end() {
    let b = StreamingBackend { event_count: 2 };
    let wo = make_work_order("lifecycle events");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    assert!(!events.is_empty());
    assert!(
        matches!(
            &events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ),
        "first event should be RunStarted"
    );
    assert!(
        matches!(
            &events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
}

#[tokio::test]
async fn run_lifecycle_event_count_matches_config() {
    let b = StreamingBackend { event_count: 5 };
    let wo = make_work_order("count events");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    // 1 RunStarted + 5 deltas + 1 RunCompleted = 7
    assert_eq!(events.len(), 7);
}

#[tokio::test]
async fn run_lifecycle_receipt_has_hash() {
    let b = StreamingBackend { event_count: 0 };
    let wo = make_work_order("hash check");
    let (receipt, _events) = run_and_collect(&b, wo).await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn run_lifecycle_receipt_contract_version() {
    let b = StreamingBackend { event_count: 0 };
    let wo = make_work_order("contract ver");
    let (receipt, _events) = run_and_collect(&b, wo).await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn run_lifecycle_rich_backend_emits_events() {
    let b = RichBackend;
    let wo = make_work_order("rich events");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    assert!(events.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Error handling — Backend error propagation
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn error_propagation_failing_backend() {
    let b = FailingBackend;
    let wo = make_work_order("error test");
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);
    let result = b.run(run_id, wo, tx).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("intentional backend failure"));
}

#[tokio::test]
async fn error_propagation_no_events_on_failure() {
    let b = FailingBackend;
    let wo = make_work_order("no events on err");
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(64);
    let _ = b.run(run_id, wo, tx).await;
    assert!(rx.try_recv().is_err(), "should have no events on failure");
}

#[tokio::test]
async fn error_propagation_dropped_sender() {
    // Verify backend handles a dropped receiver gracefully.
    let b = StreamingBackend { event_count: 3 };
    let wo = make_work_order("dropped rx");
    let run_id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    // Backend should still return a receipt even if send fails.
    let result = b.run(run_id, wo, tx).await;
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Streaming — event stream correctness
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn streaming_zero_events() {
    let b = StreamingBackend { event_count: 0 };
    let wo = make_work_order("zero events");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    // RunStarted + RunCompleted
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn streaming_single_event() {
    let b = StreamingBackend { event_count: 1 };
    let wo = make_work_order("single event");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn streaming_many_events() {
    let b = StreamingBackend { event_count: 50 };
    let wo = make_work_order("many events");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    assert_eq!(events.len(), 52); // 1 + 50 + 1
}

#[tokio::test]
async fn streaming_delta_content_correct() {
    let b = StreamingBackend { event_count: 3 };
    let wo = make_work_order("delta content");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["chunk-0", "chunk-1", "chunk-2"]);
}

#[tokio::test]
async fn streaming_events_have_timestamps() {
    let b = StreamingBackend { event_count: 2 };
    let wo = make_work_order("ts check");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    for event in &events {
        // All timestamps should be recent (within last 10 seconds).
        let diff = Utc::now() - event.ts;
        assert!(diff.num_seconds() < 10);
    }
}

#[tokio::test]
async fn streaming_events_monotonic_timestamps() {
    let b = StreamingBackend { event_count: 5 };
    let wo = make_work_order("monotonic ts");
    let (_receipt, events) = run_and_collect(&b, wo).await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts, "timestamps should be non-decreasing");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Registry — backend registration and lookup
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_register_increases_len() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

#[test]
fn registry_contains_after_register() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", sample_metadata("x", "openai"));
    assert!(reg.contains("x"));
    assert!(!reg.contains("y"));
}

#[test]
fn registry_metadata_round_trip() {
    let mut reg = BackendRegistry::new();
    let md = sample_metadata("test", "anthropic");
    reg.register_with_metadata("test", md);
    let retrieved = reg.metadata("test").unwrap();
    assert_eq!(retrieved.name, "test");
    assert_eq!(retrieved.dialect, "anthropic");
}

#[test]
fn registry_list_sorted() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("z", sample_metadata("z", "openai"));
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("m", sample_metadata("m", "openai"));
    assert_eq!(reg.list(), vec!["a", "m", "z"]);
}

#[test]
fn registry_remove_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    let removed = reg.remove("gpt4");
    assert!(removed.is_some());
    assert!(!reg.contains("gpt4"));
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = BackendRegistry::new();
    assert!(reg.remove("ghost").is_none());
}

#[test]
fn registry_by_dialect_filters_correctly() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "anthropic"));
    reg.register_with_metadata("c", sample_metadata("c", "openai"));
    assert_eq!(reg.by_dialect("openai"), vec!["a", "c"]);
    assert_eq!(reg.by_dialect("anthropic"), vec!["b"]);
    assert!(reg.by_dialect("gemini").is_empty());
}

#[test]
fn registry_healthy_backends_filter() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "openai"));
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "b",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            ..BackendHealth::default()
        },
    );
    assert_eq!(reg.healthy_backends(), vec!["a"]);
}

#[test]
fn registry_register_many_backends() {
    let mut reg = BackendRegistry::new();
    for i in 0..100 {
        let name = format!("backend-{i:03}");
        reg.register_with_metadata(&name, sample_metadata(&name, "openai"));
    }
    assert_eq!(reg.len(), 100);
    assert_eq!(reg.list().len(), 100);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Selection — choose backend based on capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ensure_capabilities_satisfied() {
    let mut caps = CapabilityManifest::default();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capabilities_unsatisfied() {
    let caps = CapabilityManifest::default();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = ensure_capability_requirements(&reqs, &caps);
    assert!(result.is_err());
}

#[test]
fn ensure_capabilities_empty_requirements_always_pass() {
    let caps = CapabilityManifest::default();
    let reqs = CapabilityRequirements::default();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capabilities_native_satisfies_emulated() {
    let mut caps = CapabilityManifest::default();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capabilities_emulated_does_not_satisfy_native() {
    let mut caps = CapabilityManifest::default();
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn ensure_capabilities_multiple_requirements_all_met() {
    let mut caps = CapabilityManifest::default();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
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
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capabilities_multiple_requirements_partial_fail() {
    let mut caps = CapabilityManifest::default();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    // ToolRead is missing entirely
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
    let result = ensure_capability_requirements(&reqs, &caps);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("ToolRead"));
}

#[test]
fn ensure_capabilities_error_lists_all_unsatisfied() {
    let caps = CapabilityManifest::default();
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
    let result = ensure_capability_requirements(&reqs, &caps);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("Streaming"));
    assert!(msg.contains("ToolBash"));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Concurrent runs — multiple backend runs simultaneously
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrent_runs_counting_backend() {
    let backend = Arc::new(CountingBackend::new());
    let mut handles = Vec::new();
    for _ in 0..10 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let wo = make_work_order("concurrent");
            let run_id = Uuid::new_v4();
            let (tx, _rx) = mpsc::channel(64);
            b.run(run_id, wo, tx).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(backend.run_count(), 10);
}

#[tokio::test]
async fn concurrent_runs_independent_receipts() {
    let backend = Arc::new(StreamingBackend { event_count: 2 });
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let wo = make_work_order(&format!("task-{i}"));
            let run_id = Uuid::new_v4();
            let (tx, _rx) = mpsc::channel(64);
            let receipt = b.run(run_id, wo, tx).await.unwrap();
            (run_id, receipt)
        }));
    }
    let mut run_ids = std::collections::HashSet::new();
    for h in handles {
        let (run_id, receipt) = h.await.unwrap();
        assert_eq!(receipt.meta.run_id, run_id);
        run_ids.insert(run_id);
    }
    assert_eq!(run_ids.len(), 5, "all run IDs should be distinct");
}

#[tokio::test]
async fn concurrent_runs_shared_dyn_backend() {
    let backend: Arc<dyn Backend> = Arc::new(StreamingBackend { event_count: 1 });
    let mut handles = Vec::new();
    for _ in 0..3 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let wo = make_work_order("shared dyn");
            let run_id = Uuid::new_v4();
            let (tx, _rx) = mpsc::channel(64);
            b.run(run_id, wo, tx).await.unwrap()
        }));
    }
    for h in handles {
        let receipt = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Metrics — registry and health tracking
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_status_default_is_unknown() {
    let h = BackendHealth::default();
    assert_eq!(h.status, HealthStatus::Unknown);
    assert!(h.last_check.is_none());
    assert!(h.latency_ms.is_none());
    assert!((h.error_rate - 0.0).abs() < f64::EPSILON);
    assert_eq!(h.consecutive_failures, 0);
}

#[test]
fn health_status_serialization_roundtrip() {
    for status in [
        HealthStatus::Healthy,
        HealthStatus::Degraded,
        HealthStatus::Unhealthy,
        HealthStatus::Unknown,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let deser: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deser, status);
    }
}

#[test]
fn backend_health_with_latency() {
    let h = BackendHealth {
        status: HealthStatus::Healthy,
        latency_ms: Some(42),
        last_check: Some(Utc::now()),
        error_rate: 0.01,
        consecutive_failures: 0,
    };
    assert_eq!(h.latency_ms, Some(42));
    assert!(h.last_check.is_some());
}

#[test]
fn registry_health_transitions() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", sample_metadata("x", "openai"));

    // Unknown → Healthy → Degraded → Unhealthy → Healthy
    for status in [
        HealthStatus::Healthy,
        HealthStatus::Degraded,
        HealthStatus::Unhealthy,
        HealthStatus::Healthy,
    ] {
        reg.update_health(
            "x",
            BackendHealth {
                status: status.clone(),
                ..BackendHealth::default()
            },
        );
        assert_eq!(reg.health("x").unwrap().status, status);
    }
}

#[test]
fn rate_limit_creation_and_access() {
    let rl = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    assert_eq!(rl.requests_per_minute, 60);
    assert_eq!(rl.tokens_per_minute, 100_000);
    assert_eq!(rl.concurrent_requests, 5);
}

#[test]
fn metadata_with_rate_limit() {
    let md = BackendMetadata {
        name: "gpt4".to_string(),
        dialect: "openai".to_string(),
        version: "2.0".to_string(),
        max_tokens: Some(128_000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: Some(RateLimit {
            requests_per_minute: 120,
            tokens_per_minute: 500_000,
            concurrent_requests: 20,
        }),
    };
    assert!(md.rate_limit.is_some());
    assert_eq!(md.rate_limit.unwrap().concurrent_requests, 20);
}

#[test]
fn metadata_serde_roundtrip() {
    let md = sample_metadata("test", "anthropic");
    let json = serde_json::to_string(&md).unwrap();
    let deser: BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.name, "test");
    assert_eq!(deser.dialect, "anthropic");
    assert_eq!(deser.max_tokens, Some(4096));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Configuration — execution mode and passthrough
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn extract_execution_mode_default_is_mapped() {
    let wo = make_work_order("default mode");
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_execution_mode_nested_passthrough() {
    let mut wo = make_work_order("passthrough nested");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_execution_mode_nested_mapped() {
    let mut wo = make_work_order("mapped nested");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "mapped"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_execution_mode_flat_key() {
    let mut wo = make_work_order("flat passthrough");
    wo.config
        .vendor
        .insert("abp.mode".to_string(), json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn extract_execution_mode_invalid_value_falls_to_default() {
    let mut wo = make_work_order("invalid mode");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "turbo"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn extract_execution_mode_nested_takes_precedence_over_flat() {
    let mut wo = make_work_order("precedence");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    wo.config
        .vendor
        .insert("abp.mode".to_string(), json!("mapped"));
    // Nested form should win.
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn validate_passthrough_compatibility_always_ok() {
    let wo = make_work_order("passthrough compat");
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

#[test]
fn execution_mode_default_trait() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let deser: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(deser, mode);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Timeout — backend timeout handling
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn timeout_slow_backend_completes_normally() {
    let b = SlowBackend {
        delay: Duration::from_millis(10),
    };
    let wo = make_work_order("slow normal");
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = b.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn timeout_backend_can_be_cancelled() {
    let b = SlowBackend {
        delay: Duration::from_secs(60),
    };
    let wo = make_work_order("cancel me");
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);

    let result = tokio::time::timeout(Duration::from_millis(50), b.run(run_id, wo, tx)).await;
    assert!(result.is_err(), "should have timed out");
}

#[tokio::test]
async fn timeout_racing_fast_vs_slow() {
    let fast = StreamingBackend { event_count: 0 };
    let slow = SlowBackend {
        delay: Duration::from_secs(60),
    };

    let wo_fast = make_work_order("fast");
    let wo_slow = make_work_order("slow");
    let (tx1, _rx1) = mpsc::channel(64);
    let (tx2, _rx2) = mpsc::channel(64);
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    let result = tokio::select! {
        r = fast.run(id1, wo_fast, tx1) => r.map(|r| r.backend.id),
        r = slow.run(id2, wo_slow, tx2) => r.map(|r| r.backend.id),
    };
    assert_eq!(result.unwrap(), "streaming", "fast backend should win");
}

#[tokio::test]
async fn timeout_multiple_slow_backends_cancelled() {
    let b = Arc::new(SlowBackend {
        delay: Duration::from_secs(60),
    });
    let mut handles = Vec::new();
    for _ in 0..5 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            let wo = make_work_order("slow multi");
            let (tx, _rx) = mpsc::channel(64);
            tokio::time::timeout(Duration::from_millis(50), b.run(Uuid::new_v4(), wo, tx)).await
        }));
    }
    for h in handles {
        assert!(h.await.unwrap().is_err(), "all should timeout");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge-case tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_fields() {
    let id = BackendIdentity {
        id: "test-id".to_string(),
        backend_version: Some("1.0".to_string()),
        adapter_version: Some("2.0".to_string()),
    };
    assert_eq!(id.id, "test-id");
    assert_eq!(id.backend_version.as_deref(), Some("1.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("2.0"));
}

#[test]
fn backend_identity_optional_fields() {
    let id = BackendIdentity {
        id: "minimal".to_string(),
        backend_version: None,
        adapter_version: None,
    };
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
}

#[test]
fn capability_manifest_operations() {
    let mut m = CapabilityManifest::default();
    assert!(m.is_empty());
    m.insert(Capability::Streaming, SupportLevel::Native);
    assert_eq!(m.len(), 1);
    assert!(m.contains_key(&Capability::Streaming));
    assert!(!m.contains_key(&Capability::ToolRead));
}

#[test]
fn support_level_satisfies_native_satisfies_both() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_emulated_only_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[tokio::test]
async fn run_with_work_order_requirements_satisfied() {
    let b = RichBackend;
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = make_work_order_with_requirements(reqs);
    let (receipt, _events) = run_and_collect(&b, wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn registry_overwrite_metadata_preserves_count() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("a", sample_metadata("a-v2", "anthropic"));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.metadata("a").unwrap().dialect, "anthropic");
}

#[test]
fn registry_update_health_unregistered() {
    let mut reg = BackendRegistry::new();
    reg.update_health(
        "phantom",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    assert_eq!(reg.health("phantom").unwrap().status, HealthStatus::Healthy);
    assert!(!reg.contains("phantom"));
}

#[test]
fn registry_remove_clears_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("x", sample_metadata("x", "openai"));
    reg.update_health(
        "x",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    reg.remove("x");
    assert!(reg.health("x").is_none());
}

#[tokio::test]
async fn counting_backend_tracks_invocations() {
    let b = CountingBackend::new();
    assert_eq!(b.run_count(), 0);
    let wo = make_work_order("count-1");
    let (tx, _rx) = mpsc::channel(64);
    b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(b.run_count(), 1);
}

#[test]
fn work_order_builder_default_requirements() {
    let wo = make_work_order("defaults");
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn ensure_capabilities_restricted_satisfies_emulated() {
    let mut caps = CapabilityManifest::default();
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".to_string(),
        },
    );
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn ensure_capabilities_restricted_does_not_satisfy_native() {
    let mut caps = CapabilityManifest::default();
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".to_string(),
        },
    );
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }],
    };
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}
