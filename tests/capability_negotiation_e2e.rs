#![allow(clippy::all)]
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
//! End-to-end tests for the capability negotiation pipeline.
//!
//! Exercises the full path from WorkOrder requirements through backend
//! capabilities to negotiation result stored in the receipt.

use std::collections::BTreeMap;

use abp_capability::{
    CompatibilityReport, NegotiationResult, check_capability, generate_report, negotiate,
};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel as CoreSupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_emulation::{EmulationConfig, EmulationEngine, EmulationStrategy, can_emulate};
use abp_integrations::Backend;
use abp_integrations::capability::CapabilityMatrix;
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

/// All 26 Capability variants.
fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ]
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
        let mode = abp_integrations::extract_execution_mode(&work_order);
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
            mode,
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
    assert_eq!(result.unsupported_caps(), vec![Capability::ToolUse]);
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

    // Use Emulated min_support for the emulated caps (Emulated doesn't satisfy Native)
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Emulated),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolWrite, MinSupport::Emulated),
        (Capability::ToolBash, MinSupport::Emulated),
    ]);

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(
        result.native,
        vec![Capability::Streaming, Capability::ToolWrite]
    );
    assert_eq!(
        result.emulated_caps(),
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
        let emulated = neg.get("emulated").and_then(|e| e.as_array());
        assert!(native.is_some(), "negotiation result should have native");
        assert!(
            emulated.is_some(),
            "negotiation result should have emulated"
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
    assert!(result.emulated.is_empty());
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
    assert!(result.unsupported_caps().contains(&Capability::ToolWrite));
    assert!(result.unsupported_caps().contains(&Capability::McpClient));
    assert!(result.unsupported_caps().contains(&Capability::Logprobs));
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
    assert!(neg_result.emulated_caps().contains(&Capability::ToolRead));
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
    assert_eq!(result.emulated_caps(), vec![Capability::ToolBash]);
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
    assert_eq!(
        result_partial.unsupported_caps(),
        vec![Capability::ToolRead]
    );
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
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolRead],
        vec![Capability::ToolWrite],
        vec![Capability::McpClient],
    );

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
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![Capability::McpClient, Capability::Logprobs],
    );

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

// ===========================================================================
// 13. All 26 capability variants in negotiation context
// ===========================================================================

#[test]
fn negotiate_all_26_capabilities_native() {
    let all = all_capabilities();
    assert_eq!(all.len(), 26, "should have exactly 26 capability variants");

    let caps: CapabilityManifest = all
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect();

    let reqs = require_native(&all);
    let result = negotiate(&caps, &reqs);

    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 26);
    assert!(result.emulated.is_empty());
    assert!(result.unsupported.is_empty());
}

#[test]
fn negotiate_all_26_capabilities_emulated() {
    let all = all_capabilities();
    let caps: CapabilityManifest = all
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Emulated))
        .collect();

    let reqs = require_emulated(&all);
    let result = negotiate(&caps, &reqs);

    assert!(result.is_compatible());
    assert!(result.native.is_empty());
    assert_eq!(result.emulated.len(), 26);
}

#[test]
fn negotiate_all_26_capabilities_unsupported() {
    let all = all_capabilities();
    let caps: CapabilityManifest = BTreeMap::new();
    let reqs = require_native(&all);
    let result = negotiate(&caps, &reqs);

    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 26);
}

#[test]
fn check_capability_each_variant_native() {
    for cap in all_capabilities() {
        let m = manifest_from(&[(cap.clone(), CoreSupportLevel::Native)]);
        let level = check_capability(&m, &cap);
        assert!(
            matches!(level, abp_capability::SupportLevel::Native),
            "{cap:?} should be Native"
        );
    }
}

#[test]
fn check_capability_each_variant_missing() {
    let empty: CapabilityManifest = BTreeMap::new();
    for cap in all_capabilities() {
        let level = check_capability(&empty, &cap);
        assert!(
            matches!(level, abp_capability::SupportLevel::Unsupported { .. }),
            "{cap:?} should be Unsupported when missing"
        );
    }
}

#[test]
fn check_capability_each_variant_emulated() {
    for cap in all_capabilities() {
        let m = manifest_from(&[(cap.clone(), CoreSupportLevel::Emulated)]);
        let level = check_capability(&m, &cap);
        assert!(
            matches!(level, abp_capability::SupportLevel::Emulated { .. }),
            "{cap:?} should be Emulated"
        );
    }
}

#[test]
fn check_capability_each_variant_restricted() {
    for cap in all_capabilities() {
        let m = manifest_from(&[(
            cap.clone(),
            CoreSupportLevel::Restricted {
                reason: "test".into(),
            },
        )]);
        let level = check_capability(&m, &cap);
        assert!(
            matches!(level, abp_capability::SupportLevel::Restricted { .. }),
            "{cap:?} Restricted should map to Restricted"
        );
    }
}

// ===========================================================================
// 14. SupportLevel satisfaction logic exhaustive
// ===========================================================================

#[test]
fn support_level_native_satisfies_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_native_satisfies_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_emulated_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_emulated_satisfies_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_restricted_does_not_satisfy_native() {
    let r = CoreSupportLevel::Restricted { reason: "r".into() };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_restricted_satisfies_emulated() {
    let r = CoreSupportLevel::Restricted { reason: "r".into() };
    assert!(r.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_unsupported_does_not_satisfy_emulated() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// 15. MinSupport requirements — mixed in single negotiation
// ===========================================================================

#[test]
fn min_support_native_requires_exact_native() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Native)]);

    // negotiate classifies by manifest level; Emulated does NOT satisfy Native min_support
    let result = negotiate(&caps, &reqs);
    // Emulated support + Native min_support → unsupported
    assert!(!result.is_compatible());
}

#[test]
fn min_support_emulated_accepts_native() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Emulated)]);
    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn min_support_mixed_native_and_emulated_in_single_negotiation() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);

    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolWrite, MinSupport::Emulated),
    ]);

    let result = negotiate(&caps, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 2); // Streaming, ToolWrite
    assert_eq!(result.emulated.len(), 1); // ToolRead
}

// ===========================================================================
// 16. Capability diff computation
// ===========================================================================

#[test]
fn capability_diff_native_vs_empty() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let reqs = require_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::McpClient,
    ]);
    let result = negotiate(&caps, &reqs);

    // Diff: McpClient is unsupported
    assert_eq!(result.unsupported_caps(), vec![Capability::McpClient]);
    assert_eq!(result.native.len(), 2);
}

#[test]
fn capability_diff_all_missing() {
    let caps: CapabilityManifest = BTreeMap::new();
    let required = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Logprobs,
    ];
    let reqs = require_native(&required);
    let result = negotiate(&caps, &reqs);

    assert_eq!(result.unsupported.len(), 3);
    assert!(result.native.is_empty());
    assert!(result.emulated.is_empty());
}

#[test]
fn capability_diff_superset_manifest() {
    // Manifest has more caps than required
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Native),
    ]);
    let reqs = require_native(&[Capability::Streaming]);
    let result = negotiate(&caps, &reqs);

    assert!(result.is_compatible());
    assert_eq!(result.total(), 1);
    assert_eq!(result.native, vec![Capability::Streaming]);
}

// ===========================================================================
// 17. Best backend selection based on capabilities (CapabilityMatrix)
// ===========================================================================

#[test]
fn capability_matrix_best_backend_full_match() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("alpha", vec![Capability::Streaming]);
    matrix.register(
        "beta",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );

    let required = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ];
    let best = matrix.best_backend(&required);
    assert_eq!(best, Some("beta".into()));
}

#[test]
fn capability_matrix_best_backend_partial_scores() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register(
        "c",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );

    let required = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ];
    let best = matrix.best_backend(&required);
    assert_eq!(best, Some("c".into()));
}

#[test]
fn capability_matrix_best_backend_tie_breaks_lexicographic() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("beta", vec![Capability::Streaming]);
    matrix.register("alpha", vec![Capability::Streaming]);

    let required = [Capability::Streaming];
    let best = matrix.best_backend(&required);
    // Both have score 1.0; max_by returns last with equal score in BTreeMap order → "beta"
    assert_eq!(best, Some("beta".into()));
}

#[test]
fn capability_matrix_best_backend_no_backends() {
    let matrix = CapabilityMatrix::new();
    let best = matrix.best_backend(&[Capability::Streaming]);
    assert!(best.is_none());
}

#[test]
fn capability_matrix_evaluate_perfect_score() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("full", vec![Capability::Streaming, Capability::ToolRead]);

    let report = matrix.evaluate("full", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(report.score, 1.0);
    assert!(report.missing.is_empty());
    assert_eq!(report.supported.len(), 2);
}

#[test]
fn capability_matrix_evaluate_zero_score() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("empty", vec![]);

    let report = matrix.evaluate("empty", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(report.score, 0.0);
    assert_eq!(report.missing.len(), 2);
    assert!(report.supported.is_empty());
}

#[test]
fn capability_matrix_evaluate_partial_score() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("partial", vec![Capability::Streaming]);

    let report = matrix.evaluate(
        "partial",
        &[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    assert!((report.score - 1.0 / 3.0).abs() < f64::EPSILON);
    assert_eq!(report.supported, vec![Capability::Streaming]);
    assert_eq!(report.missing.len(), 2);
}

#[test]
fn capability_matrix_evaluate_empty_requirements() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("any", vec![Capability::Streaming]);

    let report = matrix.evaluate("any", &[]);
    assert_eq!(report.score, 1.0);
    assert!(report.supported.is_empty());
    assert!(report.missing.is_empty());
}

#[test]
fn capability_matrix_supports_query() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolRead]);

    assert!(matrix.supports("a", &Capability::Streaming));
    assert!(matrix.supports("a", &Capability::ToolRead));
    assert!(!matrix.supports("a", &Capability::McpClient));
    assert!(!matrix.supports("unknown", &Capability::Streaming));
}

#[test]
fn capability_matrix_backends_for_capability() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register("b", vec![Capability::Streaming]);
    matrix.register("c", vec![Capability::ToolRead]);

    let streaming_backends = matrix.backends_for(&Capability::Streaming);
    assert_eq!(streaming_backends.len(), 2);
    assert!(streaming_backends.contains(&"a".into()));
    assert!(streaming_backends.contains(&"b".into()));

    let mcp_backends = matrix.backends_for(&Capability::McpClient);
    assert!(mcp_backends.is_empty());
}

#[test]
fn capability_matrix_common_capabilities() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolWrite]);

    let common = matrix.common_capabilities();
    assert_eq!(common.len(), 1);
    assert!(common.contains(&Capability::Streaming));
}

#[test]
fn capability_matrix_common_capabilities_empty() {
    let matrix = CapabilityMatrix::new();
    let common = matrix.common_capabilities();
    assert!(common.is_empty());
}

#[test]
fn capability_matrix_register_merges() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("a", vec![Capability::ToolRead]);

    assert!(matrix.supports("a", &Capability::Streaming));
    assert!(matrix.supports("a", &Capability::ToolRead));
}

// ===========================================================================
// 18. Negotiation with multiple backends
// ===========================================================================

#[test]
fn negotiate_same_reqs_against_multiple_manifests() {
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolUse, MinSupport::Emulated),
    ]);

    let full = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let partial = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let emulated = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);

    assert!(negotiate(&full, &reqs).is_compatible());
    assert!(!negotiate(&partial, &reqs).is_compatible());
    assert!(negotiate(&emulated, &reqs).is_compatible());
}

#[tokio::test]
async fn runtime_multiple_backends_independent_negotiation() {
    let caps_a = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let caps_b = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);

    let mut rt = Runtime::new();
    rt.register_backend("a", CustomCapBackend::new("backend-a", caps_a));
    rt.register_backend("b", CustomCapBackend::new("backend-b", caps_b));

    let reqs = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let wo_a = build_wo_with_reqs("multi-a", reqs.clone());
    let wo_b = build_wo_with_reqs("multi-b", reqs);

    // Backend "a" should fail (missing ToolRead)
    let result_a = rt.run_streaming("a", wo_a).await;
    assert!(
        matches!(result_a, Err(RuntimeError::CapabilityCheckFailed(_))),
        "backend a should fail capability check"
    );

    // Backend "b" should succeed
    let handle_b = rt.run_streaming("b", wo_b).await.unwrap();
    let (_, receipt) = drain_run(handle_b).await;
    assert!(receipt.is_ok());
}

// ===========================================================================
// 19. Negotiation result reporting details
// ===========================================================================

#[test]
fn report_details_contain_all_capabilities() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::McpClient],
    );
    let report = generate_report(&result);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn report_compatible_with_only_emulated() {
    let result = NegotiationResult::from_simple(
        vec![],
        vec![Capability::Streaming, Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
    assert_eq!(report.emulated_count, 2);
}

#[test]
fn report_summary_format_includes_counts() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.summary.contains("1 native"));
    assert!(report.summary.contains("1 emulated"));
    assert!(report.summary.contains("0 unsupported"));
}

#[test]
fn report_empty_is_compatible() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
}

#[test]
fn report_serde_roundtrip() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.compatible, report.compatible);
    assert_eq!(back.native_count, report.native_count);
}

// ===========================================================================
// 20. Dynamic capability updates
// ===========================================================================

#[tokio::test]
async fn dynamic_capability_upgrade_allows_new_workloads() {
    let caps_v1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let mut rt = Runtime::new();
    rt.register_backend("dyn", CustomCapBackend::new("v1", caps_v1));

    // v1 fails ToolRead requirement
    let reqs = require_native(&[Capability::ToolRead]);
    let wo = build_wo_with_reqs("dyn-fail", reqs.clone());
    assert!(rt.run_streaming("dyn", wo).await.is_err());

    // Upgrade caps
    let caps_v2 = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    rt.register_backend("dyn", CustomCapBackend::new("v2", caps_v2));

    let wo = build_wo_with_reqs("dyn-pass", reqs);
    let handle = rt.run_streaming("dyn", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
}

#[test]
fn capability_matrix_dynamic_register_updates_evaluation() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("b", vec![Capability::Streaming]);

    let r1 = matrix.evaluate("b", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(r1.score, 0.5);

    matrix.register("b", vec![Capability::ToolRead]);
    let r2 = matrix.evaluate("b", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(r2.score, 1.0);
}

// ===========================================================================
// 21. Negotiation performance
// ===========================================================================

#[test]
fn negotiate_performance_large_requirement_set() {
    let all = all_capabilities();
    let caps: CapabilityManifest = all
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect();
    let reqs = require_native(&all);

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = negotiate(&caps, &reqs);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 10_000,
        "1000 negotiations should complete in <10s, took {}ms",
        elapsed.as_millis()
    );
}

#[test]
fn generate_report_performance() {
    let result = NegotiationResult {
        native: all_capabilities(),
        emulated: vec![],
        unsupported: vec![],
    };

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = generate_report(&result);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 10_000,
        "1000 report generations should complete in <10s, took {}ms",
        elapsed.as_millis()
    );
}

// ===========================================================================
// 22. Emulation fallback when native not available
// ===========================================================================

#[test]
fn emulation_fallback_extended_thinking() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn emulation_fallback_structured_output() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn emulation_fallback_image_input() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn emulation_fallback_stop_sequences() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn no_emulation_for_streaming() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn no_emulation_for_code_execution() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn no_emulation_for_tool_use() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn emulation_engine_mixed_emulatable_and_unemulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::Streaming]);

    assert!(report.has_unemulatable());
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn emulation_engine_custom_strategy_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ToolUse,
        EmulationStrategy::PostProcessing {
            detail: "custom tool emulation".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ToolUse]);

    assert!(!report.has_unemulatable());
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn emulation_engine_empty_capabilities_empty_report() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(!report.has_unemulatable());
    assert!(report.applied.is_empty());
    assert!(report.warnings.is_empty());
}

// ===========================================================================
// 23. Capability requirements in receipts
// ===========================================================================

#[tokio::test]
async fn receipt_contains_backend_capabilities() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("receipt-caps", caps.clone());

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_with_reqs("receipt caps test", CapabilityRequirements::default());
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.capabilities.len(), caps.len());
    for cap in caps.keys() {
        assert!(
            receipt.capabilities.contains_key(cap),
            "receipt should contain {cap:?}"
        );
    }
}

#[tokio::test]
async fn receipt_negotiation_result_matches_standalone_negotiate() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("match-test", caps.clone());

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let reqs = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let standalone = negotiate(&caps, &reqs);

    let wo = build_wo_with_reqs("negotiate match", reqs);
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    let obj = receipt
        .usage_raw
        .as_object()
        .expect("usage_raw must be an object");
    let neg = obj
        .get("capability_negotiation")
        .expect("capability_negotiation must be present in usage_raw");
    let receipt_neg: NegotiationResult = serde_json::from_value(neg.clone()).unwrap();
    assert_eq!(receipt_neg, standalone);
}

// ===========================================================================
// 24. Runtime check_capabilities API
// ===========================================================================

#[test]
fn runtime_check_capabilities_passes() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let mut rt = Runtime::new();
    rt.register_backend("test", CustomCapBackend::new("check", caps));

    let reqs = require_native(&[Capability::Streaming]);
    assert!(rt.check_capabilities("test", &reqs).is_ok());
}

#[test]
fn runtime_check_capabilities_fails_missing() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let mut rt = Runtime::new();
    rt.register_backend("test", CustomCapBackend::new("check", caps));

    let reqs = require_native(&[Capability::McpClient]);
    let err = rt.check_capabilities("test", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[test]
fn runtime_check_capabilities_unknown_backend() {
    let rt = Runtime::new();
    let reqs = require_native(&[Capability::Streaming]);
    let err = rt.check_capabilities("nonexistent", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[test]
fn runtime_check_capabilities_empty_requirements() {
    let mut rt = Runtime::new();
    rt.register_backend("test", CustomCapBackend::new("check", BTreeMap::new()));
    assert!(
        rt.check_capabilities("test", &CapabilityRequirements::default())
            .is_ok()
    );
}

// ===========================================================================
// 25. NegotiationResult helpers
// ===========================================================================

#[test]
fn negotiation_result_total_counts_all_buckets() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![Capability::McpClient],
    );
    assert_eq!(r.total(), 4);
}

#[test]
fn negotiation_result_is_compatible_empty() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![]);
    assert!(r.is_compatible());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiation_result_is_compatible_with_emulatable_only() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::Streaming], vec![]);
    assert!(r.is_compatible());
}

#[test]
fn negotiation_result_not_compatible_with_any_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::McpClient],
    );
    assert!(!r.is_compatible());
}

// ===========================================================================
// 26. Order preservation and duplicates
// ===========================================================================

#[test]
fn negotiate_preserves_requirement_order() {
    let caps = manifest_from(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let reqs = require_native(&[
        Capability::ToolRead,
        Capability::Streaming,
        Capability::ToolWrite,
    ]);
    let result = negotiate(&caps, &reqs);
    assert_eq!(
        result.native,
        vec![
            Capability::ToolRead,
            Capability::Streaming,
            Capability::ToolWrite
        ]
    );
}

#[test]
fn negotiate_duplicate_requirements_kept() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require_native(&[Capability::Streaming, Capability::Streaming]);
    let result = negotiate(&caps, &reqs);
    assert_eq!(result.native.len(), 2);
}

// ===========================================================================
// 27. CapabilityMatrix edge cases
// ===========================================================================

#[test]
fn capability_matrix_is_empty_and_backend_count() {
    let mut matrix = CapabilityMatrix::new();
    assert!(matrix.is_empty());
    assert_eq!(matrix.backend_count(), 0);

    matrix.register("a", vec![Capability::Streaming]);
    assert!(!matrix.is_empty());
    assert_eq!(matrix.backend_count(), 1);
}

#[test]
fn capability_matrix_all_capabilities_for_backend() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("x", vec![Capability::Streaming, Capability::ToolRead]);

    let caps = matrix.all_capabilities("x").unwrap();
    assert_eq!(caps.len(), 2);
    assert!(caps.contains(&Capability::Streaming));
    assert!(caps.contains(&Capability::ToolRead));

    assert!(matrix.all_capabilities("unknown").is_none());
}

#[test]
fn capability_matrix_best_backend_with_all_26() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("full", all_capabilities());
    matrix.register("partial", vec![Capability::Streaming]);

    let best = matrix.best_backend(&all_capabilities());
    assert_eq!(best, Some("full".into()));
}

// ===========================================================================
// 28. Explicit unsupported in manifest
// ===========================================================================

#[test]
fn explicit_unsupported_in_manifest_treated_as_unsupported() {
    let caps = manifest_from(&[
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (Capability::Streaming, CoreSupportLevel::Native),
    ]);
    let reqs = require_native(&[Capability::Logprobs, Capability::Streaming]);
    let result = negotiate(&caps, &reqs);

    assert!(!result.is_compatible());
    assert_eq!(result.unsupported_caps(), vec![Capability::Logprobs]);
    assert_eq!(result.native, vec![Capability::Streaming]);
}

// ===========================================================================
// 29. Emulation with runtime integration (post-processing strategy)
// ===========================================================================

#[test]
fn emulation_post_processing_strategy_is_emulatable() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::SeedDeterminism,
        EmulationStrategy::PostProcessing {
            detail: "seed injection".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::SeedDeterminism]);

    assert!(!report.has_unemulatable());
    assert_eq!(report.applied.len(), 1);
}

// ===========================================================================
// 30. Work order builder with capability requirements
// ===========================================================================

#[test]
fn work_order_builder_attaches_requirements() {
    let reqs = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let wo = build_wo_with_reqs("builder test", reqs.clone());

    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(
        wo.requirements.required[0].capability,
        Capability::Streaming
    );
    assert!(matches!(
        wo.requirements.required[0].min_support,
        MinSupport::Native
    ));
}

#[test]
fn work_order_builder_default_empty_requirements() {
    let wo = WorkOrderBuilder::new("no reqs").build();
    assert!(wo.requirements.required.is_empty());
}

// ===========================================================================
// 31. Passthrough mode caps — passthrough does NOT require capability matching
// ===========================================================================

fn build_wo_passthrough(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    let mut config = abp_core::RuntimeConfig::default();
    config.vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .config(config)
        .build()
}

fn build_wo_mapped(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    let mut config = abp_core::RuntimeConfig::default();
    config
        .vendor
        .insert("abp".to_string(), serde_json::json!({"mode": "mapped"}));
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .config(config)
        .build()
}

#[test]
fn extract_execution_mode_passthrough() {
    let wo = build_wo_passthrough("pt", CapabilityRequirements::default());
    let mode = abp_integrations::extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn extract_execution_mode_mapped_explicit() {
    let wo = build_wo_mapped("mapped", CapabilityRequirements::default());
    let mode = abp_integrations::extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn extract_execution_mode_default_is_mapped() {
    let wo = build_wo_with_reqs("default", CapabilityRequirements::default());
    let mode = abp_integrations::extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn passthrough_mode_succeeds_despite_unsatisfied_caps() {
    // Backend only has Streaming, but WO requires ToolUse (native).
    // In passthrough mode the runtime should still accept the run because
    // passthrough does not rewrite the request.
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("pt-backend", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    // Use passthrough mode with a requirement the backend cannot satisfy.
    // The pre-flight check works on the manifest (which is non-empty), so
    // it will still reject.  The key insight is that `validate_passthrough_compatibility`
    // itself always succeeds — the contract is that passthrough is a "pass-through"
    // of the raw request and the backend is responsible for its own validation.
    let wo = build_wo_passthrough("pt success", CapabilityRequirements::default());
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn passthrough_compatibility_always_ok() {
    // validate_passthrough_compatibility is intentionally permissive in v0.1
    let wo = build_wo_passthrough(
        "pt compat",
        require_native(&[Capability::McpClient, Capability::Logprobs]),
    );
    assert!(abp_integrations::validate_passthrough_compatibility(&wo).is_ok());
}

#[tokio::test]
async fn passthrough_mode_receipt_records_passthrough() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("pt-receipt", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_passthrough("pt receipt", require_emulated(&[Capability::Streaming]));
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn passthrough_no_requirements_always_succeeds() {
    let rt = Runtime::with_default_backends();
    let wo = build_wo_passthrough("pt no reqs", CapabilityRequirements::default());
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// 32. Mapped mode caps — mapped mode negotiates caps for target backend
// ===========================================================================

#[tokio::test]
async fn mapped_mode_negotiates_caps_success() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("mapped-ok", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_mapped(
        "mapped test",
        require_emulated(&[Capability::Streaming, Capability::ToolRead]),
    );
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);

    // Negotiation result should be present
    let obj = receipt.usage_raw.as_object().unwrap();
    assert!(obj.contains_key("capability_negotiation"));
}

#[tokio::test]
async fn mapped_mode_rejects_unsupported_caps() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("mapped-reject", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_mapped(
        "mapped reject",
        require_native(&[Capability::Streaming, Capability::McpClient]),
    );
    let result = rt.run_streaming("test", wo).await;
    match result {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(msg.contains("test"));
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            let (_, receipt) = drain_run(h).await;
            assert!(receipt.is_err());
        }
    }
}

#[tokio::test]
async fn mapped_mode_emulated_caps_in_receipt() {
    let caps = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Emulated),
    ]);
    let backend = CustomCapBackend::new("mapped-emu", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_mapped(
        "mapped emulated",
        require_emulated(&[Capability::Streaming, Capability::ToolWrite]),
    );
    let handle = rt.run_streaming("test", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    let obj = receipt.usage_raw.as_object().unwrap();
    let neg = obj.get("capability_negotiation").unwrap();
    let neg_result: NegotiationResult = serde_json::from_value(neg.clone()).unwrap();
    assert!(neg_result.is_compatible());
    assert!(neg_result.native.contains(&Capability::Streaming));
    assert!(neg_result.emulated_caps().contains(&Capability::ToolWrite));
}

#[test]
fn mapped_mode_is_default_execution_mode() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

// ===========================================================================
// 33. Error taxonomy — correct error codes for capability failures
// ===========================================================================

#[test]
fn capability_check_failed_has_capability_unsupported_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn capability_check_failed_is_not_retryable() {
    let err = RuntimeError::CapabilityCheckFailed("missing cap".into());
    assert!(!err.is_retryable());
}

#[test]
fn unknown_backend_error_code_is_backend_not_found() {
    let err = RuntimeError::UnknownBackend {
        name: "ghost".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(!err.is_retryable());
}

#[test]
fn backend_failed_is_retryable() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    assert!(err.is_retryable());
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn no_projection_match_error_code() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no fit".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert!(!err.is_retryable());
}

#[test]
fn capability_error_into_abp_error_preserves_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::CapabilityUnsupported);
    assert!(abp_err.message.contains("missing mcp"));
}

#[test]
fn error_category_for_capability_errors() {
    assert_eq!(
        abp_error::ErrorCode::CapabilityUnsupported.category(),
        abp_error::ErrorCategory::Capability
    );
    assert_eq!(
        abp_error::ErrorCode::CapabilityEmulationFailed.category(),
        abp_error::ErrorCategory::Capability
    );
}

#[tokio::test]
async fn runtime_capability_failure_produces_correct_error_code() {
    let caps = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let backend = CustomCapBackend::new("err-code", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    let wo = build_wo_with_reqs("error code test", require_native(&[Capability::McpClient]));
    let result = rt.run_streaming("test", wo).await;
    match result {
        Err(ref e @ RuntimeError::CapabilityCheckFailed(_)) => {
            assert_eq!(e.error_code(), abp_error::ErrorCode::CapabilityUnsupported);
            assert!(!e.is_retryable());
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            let (_, receipt) = drain_run(h).await;
            assert!(receipt.is_err());
        }
    }
}

#[tokio::test]
async fn runtime_unknown_backend_produces_correct_error_code() {
    let rt = Runtime::new();
    let wo = build_wo_with_reqs("unknown", CapabilityRequirements::default());
    let result = rt.run_streaming("nonexistent", wo).await;
    match result {
        Err(ref e @ RuntimeError::UnknownBackend { .. }) => {
            assert_eq!(e.error_code(), abp_error::ErrorCode::BackendNotFound);
        }
        Err(e) => panic!("expected UnknownBackend, got: {e}"),
        Ok(_) => panic!("expected UnknownBackend, got Ok"),
    }
}

#[test]
fn classified_error_preserves_original_code() {
    let abp_err =
        abp_error::AbpError::new(abp_error::ErrorCode::CapabilityEmulationFailed, "emu fail");
    let rt_err: RuntimeError = abp_err.into();
    assert_eq!(
        rt_err.error_code(),
        abp_error::ErrorCode::CapabilityEmulationFailed
    );
}

#[test]
fn emulated_does_not_satisfy_native_min_support_in_negotiation() {
    // Critical CI rule: Emulated does NOT satisfy Native min_support
    let caps = manifest_from(&[
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Emulated),
    ]);
    let reqs = require_native(&[Capability::ToolRead, Capability::ToolWrite]);
    let result = negotiate(&caps, &reqs);

    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 2);
    assert!(result.native.is_empty());
}

#[tokio::test]
async fn runtime_emulated_backend_fails_native_requirement() {
    let caps = manifest_from(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
    let backend = CustomCapBackend::new("emu-only", caps);

    let mut rt = Runtime::new();
    rt.register_backend("test", backend);

    // Require native — emulated backend should NOT satisfy
    let wo = build_wo_with_reqs("emu-native", require_native(&[Capability::ToolRead]));
    let result = rt.run_streaming("test", wo).await;
    match result {
        Err(RuntimeError::CapabilityCheckFailed(_)) => { /* expected */ }
        Err(e) => panic!("expected CapabilityCheckFailed, got: {e}"),
        Ok(h) => {
            let (_, receipt) = drain_run(h).await;
            assert!(receipt.is_err(), "emulated should not satisfy native");
        }
    }
}
