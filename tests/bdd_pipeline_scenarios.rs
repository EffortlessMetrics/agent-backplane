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
//! BDD-style integration tests exercising the full ABP pipeline end-to-end.
//!
//! Each test reads as a given/when/then scenario covering happy paths,
//! error paths, capability negotiation, policy enforcement, and receipt
//! integrity.

use std::path::Path;

use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};
use abp_core::validate::{ValidationError, validate_receipt};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, canonical_json, receipt_hash,
    sha256_hex,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_time_later() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .build()
}

fn mock_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

/// A backend that always fails with the given message.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

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
        CapabilityManifest::default()
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

/// A backend that streams a configurable number of delta events.
#[derive(Debug, Clone)]
struct MultiEventBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for MultiEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-event".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();

        let start_ev = make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        });
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        for i in 0..self.event_count {
            let ev = make_event(AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            });
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let done_ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        trace.push(done_ev.clone());
        let _ = events_tx.send(done_ev).await;

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"note": "multi-event"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// A backend that returns a receipt with `Outcome::Failed`.
#[derive(Debug, Clone)]
struct FailOutcomeBackend;

#[async_trait]
impl Backend for FailOutcomeBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "fail-outcome".into(),
            backend_version: None,
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();

        let err_ev = make_event(AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        });
        let _ = events_tx.send(err_ev.clone()).await;

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![err_ev],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Failed,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// A backend whose capabilities include only native streaming.
#[derive(Debug, Clone)]
struct NativeOnlyBackend;

#[async_trait]
impl Backend for NativeOnlyBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "native-only".into(),
            backend_version: None,
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let now = Utc::now();
        let ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let _ = events_tx.send(ev.clone()).await;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: now,
                finished_at: now,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Passthrough,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![ev],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ===========================================================================
// 1. Happy-path scenarios
// ===========================================================================

#[tokio::test]
async fn given_openai_request_when_routed_to_mock_then_receipt_has_hash() {
    // Given: a runtime with the mock backend
    let runtime = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Summarize the codebase")
        .model("gpt-4")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    // When: the work order is routed to the mock backend
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then: the receipt has a valid SHA-256 hash
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

#[tokio::test]
async fn given_claude_request_when_routed_to_openai_then_mapped_mode_receipt() {
    // Given: a runtime with a mock backend serving as the "openai" target
    let mut runtime = Runtime::new();
    runtime.register_backend("openai-mock", MockBackend);
    let wo = WorkOrderBuilder::new("Translate Claude request to OpenAI")
        .model("claude-3-opus")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    // When: routed to the openai-mock backend
    let handle = runtime.run_streaming("openai-mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then: the receipt mode is Mapped (default)
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn given_work_order_when_backend_streams_events_then_all_events_received() {
    // Given: a backend that emits 10 delta events plus start/complete
    let mut runtime = Runtime::new();
    runtime.register_backend("multi", MultiEventBackend { event_count: 10 });
    let wo = passthrough_wo("stream test");

    // When: the work order is executed
    let handle = runtime.run_streaming("multi", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then: we receive exactly 12 events (1 start + 10 deltas + 1 complete)
    assert_eq!(events.len(), 12);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[11].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    // All delta events are in the middle
    for ev in &events[1..11] {
        assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    }

    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn given_passthrough_mode_when_same_dialect_then_no_modification() {
    // Given: a passthrough work order
    let mut runtime = Runtime::new();
    runtime.register_backend("native", NativeOnlyBackend);
    let wo = passthrough_wo("passthrough test");

    // When: executed against a native-only backend
    let handle = runtime.run_streaming("native", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then: the receipt reports passthrough mode
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn given_work_order_when_mock_backend_then_contract_version_present() {
    let runtime = Runtime::with_default_backends();
    let wo = passthrough_wo("version check");
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn given_multiple_backends_when_each_run_then_each_produces_receipt() {
    let mut runtime = Runtime::new();
    runtime.register_backend("alpha", MockBackend);
    runtime.register_backend("beta", MultiEventBackend { event_count: 3 });

    for name in &["alpha", "beta"] {
        let wo = passthrough_wo("multi-backend test");
        let handle = runtime.run_streaming(name, wo).await.unwrap();
        let (_events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[tokio::test]
async fn given_work_order_with_task_when_run_then_receipt_work_order_id_matches() {
    let runtime = Runtime::with_default_backends();
    let wo = passthrough_wo("id match");
    let wo_id = wo.id;
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn given_work_order_when_backend_returns_then_duration_non_negative() {
    let runtime = Runtime::with_default_backends();
    let wo = passthrough_wo("duration check");
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn given_mock_backend_when_run_then_identity_is_mock() {
    let runtime = Runtime::with_default_backends();
    let wo = passthrough_wo("identity check");
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn given_work_order_when_backend_streams_then_run_id_is_set() {
    let runtime = Runtime::with_default_backends();
    let wo = passthrough_wo("run-id check");
    let handle = runtime.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.meta.run_id, run_id);
}

// ===========================================================================
// 2. Error scenarios
// ===========================================================================

#[tokio::test]
async fn given_unknown_backend_when_run_then_unknown_backend_error() {
    let runtime = Runtime::new();
    let wo = passthrough_wo("unknown backend");
    let result = runtime.run_streaming("does-not-exist", wo).await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected UnknownBackend error"),
    };

    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[tokio::test]
async fn given_invalid_work_order_when_run_then_validation_error() {
    // An invalid receipt built with an empty backend id should fail validation.
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_time(),
            finished_at: fixed_time_later(),
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: String::new(), // invalid: empty
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let result = validate_receipt(&receipt);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyBackendId))
    );
}

#[tokio::test]
async fn given_backend_timeout_when_run_then_timeout_error_code() {
    // Verify the ErrorCode taxonomy classifies backend timeouts correctly.
    let code = abp_error::ErrorCode::BackendTimeout;
    assert!(code.is_retryable());
    assert_eq!(code.category(), abp_error::ErrorCategory::Backend);
}

#[tokio::test]
async fn given_unsupported_capability_when_native_required_then_early_failure() {
    // Given: a backend that only supports streaming (native)
    let mut runtime = Runtime::new();
    runtime.register_backend("limited", NativeOnlyBackend);

    // When: the work order requires ToolRead at Native level
    let requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };

    // Then: capability check fails
    let err = runtime
        .check_capabilities("limited", &requirements)
        .unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[tokio::test]
async fn given_failing_backend_when_run_then_backend_failed_error() {
    let mut runtime = Runtime::new();
    runtime.register_backend(
        "fail",
        FailingBackend {
            message: "boom".into(),
        },
    );
    let wo = passthrough_wo("fail test");
    let handle = runtime.run_streaming("fail", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;

    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn given_unknown_backend_when_check_capabilities_then_error() {
    let runtime = Runtime::new();
    let reqs = CapabilityRequirements::default();
    let result = runtime.check_capabilities("ghost", &reqs);
    assert!(result.is_err());
}

#[tokio::test]
async fn given_empty_runtime_when_list_backends_then_empty() {
    let runtime = Runtime::new();
    assert!(runtime.backend_names().is_empty());
}

#[tokio::test]
async fn given_unknown_backend_error_when_inspected_then_not_retryable() {
    let err = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn given_capability_check_failed_when_inspected_then_not_retryable() {
    let err = RuntimeError::CapabilityCheckFailed("missing tool_read".into());
    assert!(!err.is_retryable());
}

// ===========================================================================
// 3. Capability negotiation scenarios
// ===========================================================================

#[test]
fn given_all_native_caps_when_negotiated_then_compatible() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert!(result.unsatisfied.is_empty());
    assert!(result.satisfied.contains(&Capability::Streaming));
}

#[test]
fn given_emulated_cap_when_emulated_min_then_compatible() {
    let manifest = mock_manifest(); // ToolRead is Emulated
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert!(result.satisfied.contains(&Capability::ToolRead));
}

#[test]
fn given_emulated_cap_when_native_min_then_incompatible() {
    let manifest = mock_manifest(); // ToolRead is Emulated
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::ToolRead));
}

#[test]
fn given_missing_cap_when_required_then_unsupported() {
    let manifest = mock_manifest(); // No Vision
    let request = NegotiationRequest {
        required: vec![Capability::Vision],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::Vision));
}

#[test]
fn given_multiple_required_when_all_met_then_compatible() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolWrite],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
}

#[test]
fn given_multiple_required_when_one_missing_then_incompatible() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::Vision],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::Vision));
    assert!(result.satisfied.contains(&Capability::Streaming));
}

#[test]
fn given_preferred_cap_when_present_then_appears_in_bonus() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![],
        preferred: vec![Capability::Streaming],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert!(result.bonus.contains(&Capability::Streaming));
}

#[test]
fn given_preferred_cap_when_missing_then_still_compatible() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![],
        preferred: vec![Capability::Vision],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert!(!result.bonus.contains(&Capability::Vision));
}

#[test]
fn given_empty_request_when_negotiated_then_compatible() {
    let manifest = mock_manifest();
    let request = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
}

#[test]
fn given_empty_manifest_when_required_then_incompatible() {
    let manifest = CapabilityManifest::new();
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
}

#[test]
fn given_restricted_cap_when_restricted_min_then_compatible() {
    // The negotiator ranks: Native(3) > Emulated(2) > Restricted(1) > Unsupported(0).
    // A Restricted capability meets a Restricted minimum.
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let request = NegotiationRequest {
        required: vec![Capability::ToolBash],
        preferred: vec![],
        minimum_support: SupportLevel::Restricted {
            reason: "any".into(),
        },
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
}

#[test]
fn given_restricted_cap_when_native_min_then_incompatible() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let request = NegotiationRequest {
        required: vec![Capability::ToolBash],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
}

#[test]
fn given_best_match_when_multiple_manifests_then_picks_best() {
    let mut full = CapabilityManifest::new();
    full.insert(Capability::Streaming, SupportLevel::Native);
    full.insert(Capability::ToolRead, SupportLevel::Native);

    let mut partial = CapabilityManifest::new();
    partial.insert(Capability::Streaming, SupportLevel::Emulated);

    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead],
        minimum_support: SupportLevel::Emulated,
    };

    let manifests = vec![("full", full), ("partial", partial)];
    let best = CapabilityNegotiator::best_match(&request, &manifests);
    assert!(best.is_some());
    let (name, result) = best.unwrap();
    assert_eq!(name, "full");
    assert!(result.is_compatible);
}

#[test]
fn given_best_match_when_none_compatible_then_none() {
    let manifest = CapabilityManifest::new();
    let request = NegotiationRequest {
        required: vec![Capability::Vision],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    let manifests = vec![("empty", manifest)];
    let best = CapabilityNegotiator::best_match(&request, &manifests);
    assert!(best.is_none());
}

#[test]
fn given_support_level_native_when_satisfies_native_then_true() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn given_support_level_native_when_satisfies_emulated_then_true() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_support_level_emulated_when_satisfies_native_then_false() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn given_support_level_emulated_when_satisfies_emulated_then_true() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_support_level_unsupported_when_satisfies_emulated_then_false() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_support_level_unsupported_when_satisfies_native_then_false() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

// ===========================================================================
// 4. Policy scenarios
// ===========================================================================

#[test]
fn given_denied_tool_when_checked_then_policy_violation() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("bash");
    assert!(!decision.allowed);
}

#[test]
fn given_allowed_path_when_checked_then_passes() {
    let policy = PolicyProfile {
        deny_write: vec!["secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new("src/main.rs"));
    assert!(decision.allowed);
}

#[test]
fn given_denied_write_path_when_checked_then_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new("secrets/api_key.txt"));
    assert!(!decision.allowed);
}

#[test]
fn given_denied_read_path_when_checked_then_denied() {
    let policy = PolicyProfile {
        deny_read: vec![".env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(Path::new(".env"));
    assert!(!decision.allowed);
}

#[test]
fn given_empty_policy_when_any_tool_checked_then_allowed() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("anything");
    assert!(decision.allowed);
}

#[test]
fn given_allowed_tool_list_when_unlisted_tool_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let allowed = engine.can_use_tool("read");
    let denied = engine.can_use_tool("bash");
    assert!(allowed.allowed);
    assert!(!denied.allowed);
}

#[test]
fn given_both_allow_and_deny_when_tool_in_deny_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["bash".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("bash");
    assert!(!decision.allowed);
}

#[test]
fn given_deny_write_glob_when_nested_path_then_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/*.key".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new("config/ssl/cert.key"));
    assert!(!decision.allowed);
}

#[test]
fn given_deny_read_glob_when_non_matching_path_then_allowed() {
    let policy = PolicyProfile {
        deny_read: vec!["**/*.secret".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(Path::new("src/main.rs"));
    assert!(decision.allowed);
}

// ===========================================================================
// 5. Receipt scenarios
// ===========================================================================

#[test]
fn given_successful_run_when_completed_then_receipt_has_valid_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);

    // Verify hash is consistent
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(hash, &recomputed);
}

#[test]
fn given_failed_run_when_completed_then_receipt_outcome_failure() {
    let receipt = ReceiptBuilder::new("fail-backend")
        .outcome(Outcome::Failed)
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .with_hash()
        .unwrap();

    assert_eq!(receipt.outcome, Outcome::Failed);
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn given_receipt_when_tampered_then_hash_verification_fails() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .with_hash()
        .unwrap();

    let original_hash = receipt.receipt_sha256.clone().unwrap();

    // Tamper: change the outcome
    let mut tampered = receipt;
    tampered.outcome = Outcome::Failed;

    let tampered_hash = receipt_hash(&tampered).unwrap();
    assert_ne!(original_hash, tampered_hash);
}

#[test]
fn given_receipt_with_trace_when_hashed_then_deterministic() {
    let event = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };

    // Build identical receipts with the same run_id for deterministic comparison.
    let run_id = Uuid::nil();
    let base = || Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_time(),
            finished_at: fixed_time_later(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![event.clone()],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let r1 = base().with_hash().unwrap();
    let r2 = base().with_hash().unwrap();

    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn given_receipt_when_hash_field_changes_then_hash_excludes_itself() {
    let receipt = minimal_receipt();

    // Hash with no hash set
    let h1 = receipt_hash(&receipt).unwrap();

    // Hash with some hash pre-set — should give same result
    let mut receipt_with_hash = receipt;
    receipt_with_hash.receipt_sha256 = Some("bogus".into());
    let h2 = receipt_hash(&receipt_with_hash).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn given_partial_outcome_when_built_then_receipt_captures_it() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .build();

    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn given_receipt_with_artifacts_when_built_then_artifacts_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    assert_eq!(receipt.artifacts.len(), 2);
    assert_eq!(receipt.artifacts[0].kind, "patch");
    assert_eq!(receipt.artifacts[1].path, "run.log");
}

#[test]
fn given_receipt_with_verification_when_built_then_verification_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .verification(VerificationReport {
            git_diff: Some("diff --git a/foo b/foo".into()),
            git_status: Some("M foo".into()),
            harness_ok: true,
        })
        .build();

    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_some());
}

#[test]
fn given_receipt_with_usage_when_built_then_usage_fields_present() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        })
        .build();

    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.005));
}

#[test]
fn given_receipt_with_capabilities_when_built_then_caps_preserved() {
    let caps = mock_manifest();
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .capabilities(caps.clone())
        .build();

    assert_eq!(receipt.capabilities.len(), caps.len());
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn given_receipt_builder_when_mode_set_then_mode_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .mode(ExecutionMode::Passthrough)
        .build();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn given_receipt_builder_when_backend_version_set_then_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .build();

    assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn given_fail_outcome_backend_when_run_then_receipt_outcome_failed() {
    let mut runtime = Runtime::new();
    runtime.register_backend("fail-outcome", FailOutcomeBackend);
    let wo = passthrough_wo("fail outcome test");
    let handle = runtime.run_streaming("fail-outcome", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Failed);
    assert!(!events.is_empty());
}

#[test]
fn given_receipt_with_empty_backend_id_when_validated_then_error() {
    let mut receipt = minimal_receipt();
    receipt.backend.id = String::new();

    let result = validate_receipt(&receipt);
    assert!(result.is_err());
}

#[test]
fn given_valid_receipt_when_validated_then_ok() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .with_hash()
        .unwrap();

    let result = validate_receipt(&receipt);
    assert!(result.is_ok());
}

// ===========================================================================
// 6. Cross-cutting: canonical JSON and SHA-256 helpers
// ===========================================================================

#[test]
fn given_json_value_when_canonical_then_keys_sorted() {
    let json = canonical_json(&json!({"z": 1, "a": 2, "m": 3})).unwrap();
    assert!(json.starts_with(r#"{"a":2"#));
}

#[test]
fn given_bytes_when_sha256_then_64_hex_chars() {
    let hex = sha256_hex(b"hello world");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn given_same_input_when_sha256_twice_then_same_output() {
    let h1 = sha256_hex(b"deterministic");
    let h2 = sha256_hex(b"deterministic");
    assert_eq!(h1, h2);
}

#[test]
fn given_different_input_when_sha256_then_different_output() {
    let h1 = sha256_hex(b"alpha");
    let h2 = sha256_hex(b"beta");
    assert_ne!(h1, h2);
}

// ===========================================================================
// 7. Additional edge-case scenarios
// ===========================================================================

#[test]
fn given_work_order_builder_when_defaults_then_sensible() {
    let wo = WorkOrderBuilder::new("defaults test").build();

    assert_eq!(wo.task, "defaults test");
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn given_work_order_builder_when_model_set_then_config_has_model() {
    let wo = WorkOrderBuilder::new("model test").model("gpt-4").build();

    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn given_work_order_builder_when_budget_set_then_config_has_budget() {
    let wo = WorkOrderBuilder::new("budget test")
        .max_budget_usd(10.0)
        .build();

    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn given_work_order_builder_when_max_turns_set_then_config_has_turns() {
    let wo = WorkOrderBuilder::new("turns test").max_turns(20).build();

    assert_eq!(wo.config.max_turns, Some(20));
}

#[test]
fn given_work_order_builder_when_all_set_then_all_preserved() {
    let wo = WorkOrderBuilder::new("full test")
        .model("claude-3-opus")
        .max_budget_usd(5.0)
        .max_turns(10)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root("/tmp/ws")
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("claude-3-opus"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn given_execution_mode_default_then_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn given_outcome_complete_when_serialized_then_snake_case() {
    let s = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(s, r#""complete""#);
}

#[test]
fn given_outcome_failed_when_serialized_then_snake_case() {
    let s = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(s, r#""failed""#);
}

#[test]
fn given_outcome_partial_when_serialized_then_snake_case() {
    let s = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(s, r#""partial""#);
}

#[tokio::test]
async fn given_zero_events_backend_when_run_then_receipt_trace_may_be_minimal() {
    let mut runtime = Runtime::new();
    runtime.register_backend("multi-zero", MultiEventBackend { event_count: 0 });
    let wo = passthrough_wo("zero events");
    let handle = runtime.run_streaming("multi-zero", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Start + complete = 2
    assert_eq!(events.len(), 2);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn given_large_event_count_when_streamed_then_all_received() {
    let mut runtime = Runtime::new();
    runtime.register_backend("multi-100", MultiEventBackend { event_count: 100 });
    let wo = passthrough_wo("many events");
    let handle = runtime.run_streaming("multi-100", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // 1 start + 100 deltas + 1 complete = 102
    assert_eq!(events.len(), 102);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn given_receipt_duration_when_times_correct_then_positive() {
    let receipt = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .build();

    assert_eq!(receipt.meta.duration_ms, 42_000);
}

#[test]
fn given_receipt_builder_when_run_id_set_then_unique() {
    let r1 = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .build();

    // Each build generates a new run_id via Uuid::new_v4()
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[test]
fn given_error_code_taxonomy_when_backend_not_found_then_backend_category() {
    assert_eq!(
        abp_error::ErrorCode::BackendNotFound.category(),
        abp_error::ErrorCategory::Backend
    );
}

#[test]
fn given_error_code_taxonomy_when_capability_unsupported_then_capability_category() {
    assert_eq!(
        abp_error::ErrorCode::CapabilityUnsupported.category(),
        abp_error::ErrorCategory::Capability
    );
}

#[test]
fn given_error_code_taxonomy_when_policy_denied_then_policy_category() {
    assert_eq!(
        abp_error::ErrorCode::PolicyDenied.category(),
        abp_error::ErrorCategory::Policy
    );
}
