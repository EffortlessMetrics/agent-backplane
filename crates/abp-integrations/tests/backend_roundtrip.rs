// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration roundtrip tests for the Backend trait.
//!
//! These tests exercise the full pipeline: create WorkOrder → run → collect
//! events → receive Receipt → verify all fields and invariants.

use std::collections::HashSet;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrderBuilder,
};
use abp_integrations::{Backend, MockBackend};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
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

// ---------------------------------------------------------------------------
// Custom failing backend for error-handling tests
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Custom backend that reports failure outcome in receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FailureOutcomeBackend;

#[async_trait]
impl Backend for FailureOutcomeBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failure_outcome".to_string(),
            backend_version: Some("0.1".to_string()),
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let started = chrono::Utc::now();

        let err_event = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: "something went wrong".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(err_event.clone()).await;

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        let receipt = Receipt {
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
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![err_event],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Failed,
            receipt_sha256: None,
        }
        .with_hash()?;

        Ok(receipt)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. MockBackend roundtrip: create work order → run → receipt → verify fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_mock_backend_all_fields() {
    let backend = MockBackend;
    let wo = make_work_order("roundtrip test");
    let wo_id = wo.id;
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();

    // Meta
    assert_eq!(receipt.meta.run_id, run_id);
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);

    // Backend
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));

    // Capabilities present
    assert!(!receipt.capabilities.is_empty());

    // Mode
    assert_eq!(receipt.mode, ExecutionMode::Mapped);

    // Usage
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));

    // Trace non-empty
    assert!(!receipt.trace.is_empty());

    // Outcome
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Hash
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest should be 64 chars");
}

// ---------------------------------------------------------------------------
// 2. Event streaming: collect events and verify stream order
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_event_stream_order() {
    let backend = MockBackend;
    let wo = make_work_order("event order test");
    let (receipt, events) = run_and_collect(&backend, wo).await;

    assert!(events.len() >= 2, "expected at least 2 streamed events");

    // First event must be RunStarted
    assert!(
        matches!(
            events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ),
        "first streamed event should be RunStarted"
    );
    // Last event must be RunCompleted
    assert!(
        matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last streamed event should be RunCompleted"
    );

    // Timestamps should be monotonically non-decreasing
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "event timestamps should be non-decreasing"
        );
    }

    // Trace in receipt should have same length
    assert_eq!(receipt.trace.len(), events.len());
}

// ---------------------------------------------------------------------------
// 3. Receipt hash: run → receipt has valid, recomputable hash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_receipt_hash_valid() {
    let backend = MockBackend;
    let wo = make_work_order("hash test");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    let stored = receipt.receipt_sha256.clone().expect("hash should exist");
    assert_eq!(stored.len(), 64);

    // Recompute and verify
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(
        stored, recomputed,
        "hash should be deterministically reproducible"
    );
}

// ---------------------------------------------------------------------------
// 4. Multiple sequential runs: 5 work orders → all unique receipts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_sequential_runs_unique_receipts() {
    let backend = MockBackend;
    let mut run_ids = HashSet::new();
    let mut hashes = HashSet::new();

    for i in 0..5 {
        let wo = make_work_order(&format!("sequential task {i}"));
        let (receipt, _) = run_and_collect(&backend, wo).await;

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(
            run_ids.insert(receipt.meta.run_id),
            "run_id should be unique across sequential runs"
        );
        if let Some(h) = &receipt.receipt_sha256 {
            hashes.insert(h.clone());
        }
    }

    assert_eq!(run_ids.len(), 5);
    // Hashes should all be unique since tasks differ
    assert_eq!(hashes.len(), 5, "each receipt should have a unique hash");
}

// ---------------------------------------------------------------------------
// 5. Concurrent runs: 3 work orders with separate MockBackends
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_concurrent_runs_separate_backends() {
    let handles: Vec<_> = (0..3)
        .map(|i| {
            tokio::spawn(async move {
                let backend = MockBackend;
                let wo = make_work_order(&format!("concurrent task {i}"));
                let (tx, mut rx) = mpsc::channel(64);
                let run_id = Uuid::new_v4();
                let receipt = backend.run(run_id, wo, tx).await.unwrap();
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                (receipt, events)
            })
        })
        .collect();

    let mut seen_run_ids = HashSet::new();
    for h in handles {
        let (receipt, events) = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(!events.is_empty());
        assert!(
            seen_run_ids.insert(receipt.meta.run_id),
            "concurrent runs should produce unique run_ids"
        );
    }
    assert_eq!(seen_run_ids.len(), 3);
}

// ---------------------------------------------------------------------------
// 6. Empty task: still produces valid receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_empty_task_produces_receipt() {
    let backend = MockBackend;
    let wo = make_work_order("");
    let (receipt, events) = run_and_collect(&backend, wo).await;

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    assert!(
        !events.is_empty(),
        "events should still be emitted for empty task"
    );
}

// ---------------------------------------------------------------------------
// 7. Large task: 10KB task → receipt preserves work order info
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_large_task_preserves_info() {
    let backend = MockBackend;
    let large_task = "A".repeat(10 * 1024); // 10KB
    let wo = make_work_order(&large_task);
    let wo_id = wo.id;
    let (receipt, events) = run_and_collect(&backend, wo).await;

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert!(receipt.receipt_sha256.is_some());

    // RunStarted message should contain part of the large task
    let first = events.first().unwrap();
    if let AgentEventKind::RunStarted { message } = &first.kind {
        assert!(
            message.contains(&large_task[..50]),
            "RunStarted should reference the task"
        );
    } else {
        panic!("first event should be RunStarted");
    }
}

// ---------------------------------------------------------------------------
// 8. Custom config: vendor config passes through to receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_custom_vendor_config() {
    let backend = MockBackend;
    let mut wo = make_work_order("custom config test");
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    wo.config.model = Some("gpt-4-test".to_string());
    wo.config.max_turns = Some(42);

    let (receipt, _) = run_and_collect(&backend, wo).await;

    // Passthrough mode should be reflected in receipt
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ---------------------------------------------------------------------------
// 9. Capability manifest: MockBackend reports correct capabilities
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_capability_manifest_in_receipt() {
    let backend = MockBackend;
    let wo = make_work_order("capability test");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    let caps = &receipt.capabilities;
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));

    // Capabilities in receipt should match identity query
    let direct_caps = backend.capabilities();
    assert_eq!(caps.len(), direct_caps.len());
}

// ---------------------------------------------------------------------------
// 10. Error handling: FailingBackend returns Err, FailureOutcomeBackend
//     returns receipt with Failed outcome
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_backend_returns_error() {
    let backend = FailingBackend;
    let wo = make_work_order("should fail");
    let (tx, _rx) = mpsc::channel(64);

    let result = backend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("intentional backend failure"),
        "error message should describe the failure"
    );
}

#[tokio::test]
async fn roundtrip_backend_failure_outcome() {
    let backend = FailureOutcomeBackend;
    let wo = make_work_order("should produce failed receipt");
    let (receipt, events) = run_and_collect(&backend, wo).await;

    assert_eq!(receipt.outcome, Outcome::Failed);
    assert!(receipt.receipt_sha256.is_some());

    // The error event should have been streamed
    assert!(!events.is_empty());
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::Error { .. })),
        "should have streamed an Error event"
    );
}

// ---------------------------------------------------------------------------
// 11. Event count: stream count matches receipt trace count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_event_count_matches_trace() {
    let backend = MockBackend;
    let wo = make_work_order("event count test");
    let (receipt, events) = run_and_collect(&backend, wo).await;

    assert_eq!(
        receipt.trace.len(),
        events.len(),
        "receipt trace length should equal streamed event count"
    );

    // MockBackend emits exactly 4 events: RunStarted, 2×AssistantMessage, RunCompleted
    assert_eq!(events.len(), 4, "MockBackend should emit exactly 4 events");
}

// ---------------------------------------------------------------------------
// 12. Backend identity: receipt backend_id matches backend name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_backend_identity_matches() {
    let backend = MockBackend;
    let wo = make_work_order("identity test");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    let expected_identity = backend.identity();
    assert_eq!(receipt.backend.id, expected_identity.id);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(
        receipt.backend.backend_version,
        expected_identity.backend_version
    );
    assert_eq!(
        receipt.backend.adapter_version,
        expected_identity.adapter_version
    );
}

// ---------------------------------------------------------------------------
// 13. Contract version: receipt matches CONTRACT_VERSION constant
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_contract_version_matches() {
    let backend = MockBackend;
    let wo = make_work_order("contract version test");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.meta.contract_version, "abp/v0.1");
}

// ---------------------------------------------------------------------------
// 14. Work order ID propagation: receipt work_order_id matches input
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_work_order_id_propagation() {
    let backend = MockBackend;
    let wo = make_work_order("id propagation test");
    let wo_id = wo.id;
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.run_id, run_id);
    // run_id and work_order_id should be different
    assert_ne!(receipt.meta.run_id, receipt.meta.work_order_id);
}

// ---------------------------------------------------------------------------
// 15. Timing: receipt wall_time_ms is reasonable (>=0, <30000)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_timing_reasonable() {
    let backend = MockBackend;
    let wo = make_work_order("timing test");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    // duration_ms should be non-negative and well under 30 seconds for a mock
    assert!(
        receipt.meta.duration_ms < 30_000,
        "mock backend should complete in under 30s, got {}ms",
        receipt.meta.duration_ms
    );

    // Verify finished_at >= started_at
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);

    // Verify duration_ms is consistent with timestamps
    let delta = (receipt.meta.finished_at - receipt.meta.started_at)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    assert_eq!(
        receipt.meta.duration_ms, delta,
        "duration_ms should match timestamp difference"
    );
}

// ---------------------------------------------------------------------------
// 16. Receipt serialization roundtrip: serialize → deserialize → verify
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_receipt_serde() {
    let backend = MockBackend;
    let wo = make_work_order("serde roundtrip");
    let (receipt, _) = run_and_collect(&backend, wo).await;

    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.meta.run_id, receipt.meta.run_id);
    assert_eq!(deserialized.meta.work_order_id, receipt.meta.work_order_id);
    assert_eq!(deserialized.backend.id, receipt.backend.id);
    assert_eq!(deserialized.outcome, receipt.outcome);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(deserialized.trace.len(), receipt.trace.len());
    assert_eq!(deserialized.mode, receipt.mode);
}

// ---------------------------------------------------------------------------
// 17. Unsatisfied capability requirement: run returns error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_unsatisfied_capability_rejects() {
    let backend = MockBackend;
    let mut wo = make_work_order("unsatisfied cap");
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let (tx, _rx) = mpsc::channel(64);
    let result = backend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("capability requirements not satisfied")
    );
}

// ---------------------------------------------------------------------------
// 18. Satisfied capability requirement: run succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_satisfied_capability_succeeds() {
    let backend = MockBackend;
    let mut wo = make_work_order("satisfied cap");
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let (receipt, _) = run_and_collect(&backend, wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ---------------------------------------------------------------------------
// 19. Hash determinism: same receipt data → same hash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_hash_determinism() {
    let backend = MockBackend;

    // Run twice — hashes differ because timestamps/UUIDs differ
    let wo1 = make_work_order("hash determinism");
    let (r1, _) = run_and_collect(&backend, wo1).await;

    // But recomputing the same receipt's hash should be stable
    let hash1 = abp_core::receipt_hash(&r1).unwrap();
    let hash2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(
        hash1, hash2,
        "recomputing hash on same receipt should be deterministic"
    );
    assert_eq!(
        r1.receipt_sha256.as_ref().unwrap(),
        &hash1,
        "stored hash should match recomputed"
    );
}

// ---------------------------------------------------------------------------
// 20. Trace events have ext = None for mock backend
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roundtrip_trace_events_ext_none() {
    let backend = MockBackend;
    let wo = make_work_order("ext field test");
    let (receipt, events) = run_and_collect(&backend, wo).await;

    for (i, ev) in events.iter().enumerate() {
        assert!(
            ev.ext.is_none(),
            "event {i} ext should be None for mock backend"
        );
    }
    for (i, ev) in receipt.trace.iter().enumerate() {
        assert!(
            ev.ext.is_none(),
            "trace event {i} ext should be None for mock backend"
        );
    }
}
