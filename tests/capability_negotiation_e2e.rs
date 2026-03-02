// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the capability negotiation pipeline.
//!
//! Exercises the full path from WorkOrder requirements through backend
//! capabilities to negotiation result stored in the receipt.

use std::collections::BTreeMap;

use abp_capability::{NegotiationResult, generate_report, negotiate};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel as CoreSupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationStrategy};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a RunHandle.
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("backend task panicked");
    (collected, receipt)
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

/// A custom backend with configurable capabilities.
#[derive(Debug, Clone)]
struct CustomCapBackend {
    name: String,
    caps: CapabilityManifest,
}

impl CustomCapBackend {
    fn new(name: &str, caps: CapabilityManifest) -> Self {
        Self {
            name: name.into(),
            caps,
        }
    }
}

#[async_trait]
impl Backend for CustomCapBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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

        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(start_ev.clone()).await;

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(end_ev.clone()).await;

        let finished = chrono::Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized::default(),
            trace: vec![start_ev, end_ev],
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

fn build_wo_with_reqs(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build()
}

// ===========================================================================
// 1. Streaming requirement → backend supports streaming → passes
// ===========================================================================

#[tokio::test]
async fn streaming_requirement_satisfied_by_native_support() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("streaming-native", caps.clone());

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_native(&[Capability::Streaming]);
    let wo = build_wo_with_reqs("streaming test", reqs.clone());

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // Verify negotiation passed at the capability layer
    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

// ===========================================================================
// 2. ToolUse requirement → backend missing → negotiation fails
// ===========================================================================

#[tokio::test]
async fn tool_use_requirement_missing_from_backend_fails() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("no-tool-use", caps.clone());

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_native(&[Capability::ToolUse]);
    let wo = build_wo_with_reqs("tool_use test", reqs.clone());

    let handle = rt.run_streaming("test", wo).await;
    // Pre-flight check should reject
    match handle {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(msg.contains("test"), "error should mention backend name");
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            // Runtime may defer check to inside the run task
            let (_events, receipt) = drain_run(h).await;
            assert!(
                receipt.is_err(),
                "run should fail when ToolUse is unsupported"
            );
        }
    }

    // Verify at negotiation layer
    let result = negotiate(&caps, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported, vec![Capability::ToolUse]);
}

// ===========================================================================
// 3. Optional caps → all provided → full compatibility report
// ===========================================================================

#[test]
fn full_compatibility_report_all_caps_provided() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Emulated),
        (Capability::ToolBash, CoreSupportLevel::Native),
    ]);

    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolWrite, MinSupport::Emulated),
        (Capability::ToolBash, MinSupport::Native),
    ]);

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.total(), 4);

    let report = generate_report(&result);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
    assert_eq!(report.details.len(), 4);
    assert_eq!(report.native_count, 3); // Streaming, ToolRead, ToolBash
    assert_eq!(report.emulated_count, 1); // ToolWrite
    assert_eq!(report.unsupported_count, 0);
}

// ===========================================================================
// 4. Emulatable caps → emulation engine satisfies → passes with emulated label
// ===========================================================================

#[tokio::test]
async fn emulatable_caps_satisfied_via_emulation_engine() {
    // Backend has Streaming natively, but not ExtendedThinking
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("partial-cap", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    // Configure emulation for ExtendedThinking
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );
    let rt = rt.with_emulation(emu_config);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ExtendedThinking]);
    let wo = build_wo_with_reqs("emulation test", reqs);

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // Verify emulation report is recorded in receipt metadata
    if let Some(obj) = receipt.usage_raw.as_object()
        && let Some(emu) = obj.get("emulation")
    {
        let applied = emu.get("applied").and_then(|a| a.as_array());
        assert!(
            applied.is_some_and(|a| !a.is_empty()),
            "emulation report should have applied entries"
        );
    }
}

#[test]
fn emulation_engine_check_missing_reports_emulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);

    assert!(!report.has_unemulatable());
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

// ===========================================================================
// 5. Multiple caps required — some native, some emulated → mixed result
// ===========================================================================

#[test]
fn mixed_negotiation_native_and_emulated() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Emulated),
    ]);

    let reqs = require_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolBash,
    ]);

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(
        result.native,
        vec![Capability::Streaming, Capability::ToolWrite]
    );
    assert_eq!(
        result.emulatable,
        vec![Capability::ToolRead, Capability::ToolBash]
    );
    assert!(result.unsupported.is_empty());
    assert_eq!(result.total(), 4);

    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 2);
}

#[tokio::test]
async fn mixed_native_emulated_runtime_e2e() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("mixed", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("mixed caps test", reqs);

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // Negotiation result should be stored in receipt
    if let Some(obj) = receipt.usage_raw.as_object()
        && let Some(neg) = obj.get("capability_negotiation")
    {
        let native = neg.get("native").and_then(|n| n.as_array());
        let emulatable = neg.get("emulatable").and_then(|e| e.as_array());
        assert!(native.is_some(), "negotiation result should have native");
        assert!(
            emulatable.is_some(),
            "negotiation result should have emulatable"
        );
    }
}

// ===========================================================================
// 6. No caps required → always succeeds
// ===========================================================================

#[test]
fn no_requirements_always_compatible() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = CapabilityRequirements::default();

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.total(), 0);
}

#[test]
fn no_requirements_empty_manifest_also_compatible() {
    let caps: CapabilityManifest = BTreeMap::new();
    let reqs = CapabilityRequirements::default();

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.total(), 0);
}

#[tokio::test]
async fn no_requirements_runtime_e2e() {
    let rt = Runtime::with_default_backends();
    let wo = build_wo_with_reqs("no reqs", CapabilityRequirements::default());

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 7. All caps required → only fully capable backend passes
// ===========================================================================

#[test]
fn all_caps_required_fully_capable_passes() {
    let all_caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ];

    let caps: CapabilityManifest = all_caps
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect();

    let reqs = require_native(&all_caps);
    let result = negotiate(&caps, &reqs);

    assert!(result.is_compatible());
    assert_eq!(result.native.len(), all_caps.len());
    assert!(result.emulatable.is_empty());
    assert!(result.unsupported.is_empty());
}

#[test]
fn all_caps_required_partial_backend_fails() {
    let required = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::McpClient,
        Capability::Logprobs,
    ];

    // Backend only has Streaming + ToolRead
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);

    let reqs = require_native(&required);
    let result = negotiate(&caps, &reqs);

    assert!(!result.is_compatible());
    assert_eq!(result.native.len(), 2);
    assert_eq!(result.unsupported.len(), 3);
    assert!(result.unsupported.contains(&Capability::ToolWrite));
    assert!(result.unsupported.contains(&Capability::McpClient));
    assert!(result.unsupported.contains(&Capability::Logprobs));
}

#[tokio::test]
async fn all_caps_required_runtime_rejects_partial_backend() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("partial", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::McpClient,
    ]);
    let wo = build_wo_with_reqs("all caps test", reqs);

    let result = rt.run_streaming("test", wo).await;
    match result {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(
                msg.contains("test"),
                "error should reference backend name: {msg}"
            );
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            let (_events, receipt) = drain_run(h).await;
            assert!(receipt.is_err(), "run should fail with missing caps");
        }
    }
}

// ===========================================================================
// 8. Negotiation result stored in receipt metadata
// ===========================================================================

#[tokio::test]
async fn negotiation_result_stored_in_receipt_metadata() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("receipt-test", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("receipt metadata test", reqs);

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // The runtime writes capability_negotiation into usage_raw
    let obj = receipt
        .usage_raw
        .as_object()
        .expect("usage_raw should be an object");
    let neg = obj
        .get("capability_negotiation")
        .expect("should contain capability_negotiation key");

    // Deserialize back to verify structure
    let neg_result: NegotiationResult =
        serde_json::from_value(neg.clone()).expect("should deserialize to NegotiationResult");
    assert!(neg_result.is_compatible());
    assert!(neg_result.native.contains(&Capability::Streaming));
    assert!(neg_result.emulatable.contains(&Capability::ToolRead));
    assert!(neg_result.unsupported.is_empty());
}

#[tokio::test]
async fn receipt_hash_covers_negotiation_metadata() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("hash-test", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("receipt hash test", reqs);

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt should have a hash"
    );

    // Re-hash and verify consistency
    let hash = receipt.receipt_sha256.as_ref().unwrap().clone();
    let recomputed = abp_receipt::compute_hash(&receipt).unwrap();
    assert_eq!(
        hash, recomputed,
        "hash should be stable across recomputation"
    );
}

// ===========================================================================
// 9. Capability version compatibility check
// ===========================================================================

#[test]
fn capability_version_in_contract() {
    // Verify the contract version is used consistently
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn restricted_capability_treated_as_emulated_in_negotiation() {
    let caps = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    )]);

    let reqs = require_emulated(&[Capability::ToolBash]);
    let result = negotiate(&caps, &reqs);

    assert!(result.is_compatible());
    assert_eq!(result.emulatable, vec![Capability::ToolBash]);
    assert!(result.native.is_empty());
}

#[test]
fn support_level_satisfies_hierarchy() {
    // Native satisfies both Native and Emulated min-support
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));

    // Emulated satisfies Emulated but not Native
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));

    // Restricted satisfies Emulated but not Native
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));

    // Unsupported satisfies nothing
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// 10. Backend declares caps via hello → runtime captures → negotiation uses
// ===========================================================================

#[tokio::test]
async fn backend_caps_declared_in_manifest_used_by_runtime() {
    // MockBackend declares specific capabilities in its manifest
    let rt = Runtime::with_default_backends();
    let mock_caps = rt.backend("mock").unwrap().capabilities();

    // Verify MockBackend has Streaming natively
    assert!(
        matches!(
            mock_caps.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ),
        "MockBackend should declare Streaming as Native"
    );

    // Build a work order requiring only what mock provides
    let reqs = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("mock cap test", reqs);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // Check negotiation result in receipt
    if let Some(obj) = receipt.usage_raw.as_object() {
        let neg = obj.get("capability_negotiation");
        assert!(
            neg.is_some(),
            "mock backend should produce negotiation result"
        );
    }
}

#[tokio::test]
async fn empty_manifest_backend_skips_negotiation() {
    // Backend with empty manifest (like uninitialized sidecar) — runtime
    // should skip pre-flight checks entirely.
    let backend = CustomCapBackend::new("empty-manifest", BTreeMap::new());

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_native(&[Capability::Streaming]);
    let wo = build_wo_with_reqs("empty manifest test", reqs);

    // Should NOT fail at pre-flight since manifest is empty (sidecar-like)
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // No negotiation result since manifest was empty
    if let Some(obj) = receipt.usage_raw.as_object() {
        assert!(
            obj.get("capability_negotiation").is_none(),
            "empty manifest should skip negotiation"
        );
    }
}

// ===========================================================================
// 11. Re-negotiation on backend change
// ===========================================================================

#[tokio::test]
async fn different_backends_produce_different_negotiation_results() {
    let caps_full = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let caps_partial = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);

    let reqs = require_native(&[Capability::Streaming, Capability::ToolRead]);

    // Full backend passes
    let result_full = negotiate(&caps_full, &reqs);
    assert!(result_full.is_compatible());
    assert_eq!(result_full.native.len(), 2);

    // Partial backend fails
    let result_partial = negotiate(&caps_partial, &reqs);
    assert!(!result_partial.is_compatible());
    assert_eq!(result_partial.unsupported, vec![Capability::ToolRead]);
}

#[tokio::test]
async fn runtime_re_register_backend_uses_new_caps() {
    let caps_v1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let caps_v2 = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);

    let mut rt = Runtime::new();

    // Register v1 backend
    rt.register_backend("test", CustomCapBackend::new("v1", caps_v1));

    // v1 can satisfy Streaming only
    let reqs_streaming = require_native(&[Capability::Streaming]);
    let wo = build_wo_with_reqs("v1 test", reqs_streaming);
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());

    // Re-register with v2 backend (more caps)
    rt.register_backend("test", CustomCapBackend::new("v2", caps_v2));

    // v2 can now satisfy both
    let reqs_both = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("v2 test", reqs_both);
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
}

// ===========================================================================
// 12. Concurrent negotiation for multiple work orders
// ===========================================================================

#[tokio::test]
async fn concurrent_negotiations_are_independent() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("concurrent", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    // Launch multiple work orders concurrently
    let reqs1 = require_emulated(&[Capability::Streaming]);
    let reqs2 = require_emulated(&[Capability::ToolRead]);
    let reqs3 = require_emulated(&[Capability::Streaming, Capability::ToolRead]);

    let wo1 = build_wo_with_reqs("concurrent-1", reqs1);
    let wo2 = build_wo_with_reqs("concurrent-2", reqs2);
    let wo3 = build_wo_with_reqs("concurrent-3", reqs3);

    let h1 = rt.run_streaming("test", wo1).await.unwrap();
    let h2 = rt.run_streaming("test", wo2).await.unwrap();
    let h3 = rt.run_streaming("test", wo3).await.unwrap();

    let (r1, r2, r3) = tokio::join!(drain_run(h1), drain_run(h2), drain_run(h3));

    let receipt1 = r1.1.unwrap();
    let receipt2 = r2.1.unwrap();
    let receipt3 = r3.1.unwrap();

    assert_eq!(receipt1.outcome, Outcome::Complete);
    assert_eq!(receipt2.outcome, Outcome::Complete);
    assert_eq!(receipt3.outcome, Outcome::Complete);

    // Each receipt should have its own negotiation result
    for receipt in [&receipt1, &receipt2, &receipt3] {
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[test]
fn concurrent_negotiate_calls_produce_consistent_results() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);

    let reqs = require_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]);

    // Call negotiate multiple times, results should be identical
    let results: Vec<_> = (0..10).map(|_| negotiate(&caps, &reqs)).collect();
    for r in &results {
        assert_eq!(r, &results[0], "negotiate should be deterministic");
    }
}

// ===========================================================================
// Additional edge cases
// ===========================================================================

#[test]
fn negotiation_result_serde_roundtrip() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolRead],
        emulatable: vec![Capability::ToolWrite],
        unsupported: vec![Capability::McpClient],
    };

    let json = serde_json::to_string(&result).unwrap();
    let deserialized: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, deserialized);
}

#[test]
fn emulation_config_overrides_disable_emulation() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user opted out".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);

    assert!(report.has_unemulatable());
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn emulation_unemulatable_capability_produces_warning() {
    let engine = EmulationEngine::with_defaults();
    // Streaming cannot be emulated by default
    let report = engine.check_missing(&[Capability::Streaming]);

    assert!(report.has_unemulatable());
    assert!(report.applied.is_empty());
    assert!(!report.warnings.is_empty());
}

#[tokio::test]
async fn emulation_for_unemulatable_cap_rejected_by_runtime() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("no-code-exec", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    // Enable emulation, but CodeExecution is Disabled by default
    let rt = rt.with_emulation(EmulationConfig::new());

    let reqs = require_emulated(&[Capability::Streaming, Capability::CodeExecution]);
    let wo = build_wo_with_reqs("unemulatable test", reqs);

    let result = rt.run_streaming("test", wo).await;
    match result {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(
                msg.contains("emulation unavailable"),
                "should mention emulation unavailable: {msg}"
            );
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            let (_events, receipt) = drain_run(h).await;
            assert!(
                receipt.is_err(),
                "should fail when unemulatable cap is required"
            );
        }
    }
}

#[tokio::test]
async fn emulation_report_stored_in_receipt_usage_raw() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("emu-receipt", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think carefully.".into(),
        },
    );
    let rt = rt.with_emulation(emu_config);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ExtendedThinking]);
    let wo = build_wo_with_reqs("emu receipt test", reqs);

    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    let obj = receipt.usage_raw.as_object().expect("should be object");
    let emu = obj.get("emulation").expect("should have emulation key");
    let applied = emu.get("applied").and_then(|a| a.as_array());
    assert!(applied.is_some_and(|a| !a.is_empty()));
}

#[test]
fn generate_report_reflects_mixed_negotiation() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulatable: vec![Capability::ToolRead, Capability::ToolWrite],
        unsupported: vec![Capability::McpClient, Capability::Logprobs],
    };

    let report = generate_report(&result);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 2);
    assert_eq!(report.unsupported_count, 2);
    assert_eq!(report.details.len(), 5);
}

#[tokio::test]
async fn receipt_chain_accumulates_across_runs() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("chain-test", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_native(&[Capability::Streaming]);

    // Run twice
    let wo1 = build_wo_with_reqs("chain run 1", reqs.clone());
    let h1 = rt.run_streaming("test", wo1).await.unwrap();
    let (_, r1) = drain_run(h1).await;
    assert!(r1.is_ok());

    let wo2 = build_wo_with_reqs("chain run 2", reqs);
    let h2 = rt.run_streaming("test", wo2).await.unwrap();
    let (_, r2) = drain_run(h2).await;
    assert!(r2.is_ok());

    // Verify chain has 2 receipts
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 2, "receipt chain should have 2 entries");
}
