// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pipeline integration tests: config → runtime → backend → receipt.
//!
//! 25+ tests exercising the full ABP stack across five categories:
//! 1. Full pipeline
//! 2. Backend selection
//! 3. Error propagation
//! 4. Receipt chain
//! 5. Cross-crate integration

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    receipt_hash,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_emulation::{EmulationConfig, EmulationStrategy};
use abp_integrations::Backend;
use abp_mapping::{MappingMatrix, known_rules};
use abp_policy::PolicyEngine;
use abp_runtime::store::ReceiptStore;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a [`RunHandle`].
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

/// Run a mock pipeline and return the receipt.
async fn run_mock_receipt(rt: &Runtime, task: &str) -> Receipt {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    receipt.unwrap()
}

/// A backend that always returns an error.
#[derive(Debug, Clone)]
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
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("intentional failure for testing")
    }
}

/// A backend with a custom identity and configurable capabilities.
#[derive(Debug, Clone)]
struct IdentityBackend {
    name: String,
    version: String,
    caps: CapabilityManifest,
}

#[async_trait]
impl Backend for IdentityBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some(self.version.clone()),
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
        let mut trace = Vec::new();

        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.name),
            },
            ext: None,
        };
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(end_ev.clone());
        let _ = events_tx.send(end_ev).await;

        let finished = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
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
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

// ===========================================================================
// 1. Full pipeline (5 tests)
// ===========================================================================

#[tokio::test]
async fn full_pipeline_mock_backend_produces_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("pipeline test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(!events.is_empty());
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn full_pipeline_custom_config_propagates_settings() {
    let config = RuntimeConfig {
        model: Some("test-model".into()),
        max_budget_usd: Some(5.0),
        max_turns: Some(10),
        ..Default::default()
    };

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("config propagation")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("test-model"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(10));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn full_pipeline_collects_events_and_hashed_receipt() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock_receipt(&rt, "events + hash").await;

    assert!(!receipt.trace.is_empty());
    let hash = receipt.receipt_sha256.as_ref().expect("hash present");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());
}

#[tokio::test]
async fn full_pipeline_passthrough_mode_propagates() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn full_pipeline_policy_profile_attached() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/secret/**".into()],
        ..PolicyProfile::default()
    };

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy pipeline")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy.clone())
        .build();

    // Policy compiles and pipeline completes.
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_write_path(Path::new("secret/data.txt")).allowed);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 2. Backend selection (5 tests)
// ===========================================================================

#[tokio::test]
async fn backend_register_and_select_by_name() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);

    let receipt = run_mock_receipt(&rt, "select by name").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn backend_register_multiple_select_correct() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "alpha",
        IdentityBackend {
            name: "alpha".into(),
            version: "1.0".into(),
            caps: CapabilityManifest::default(),
        },
    );
    rt.register_backend(
        "beta",
        IdentityBackend {
            name: "beta".into(),
            version: "2.0".into(),
            caps: CapabilityManifest::default(),
        },
    );

    let wo = WorkOrderBuilder::new("select beta")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("beta", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.backend.id, "beta");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0"));
}

#[tokio::test]
async fn backend_unknown_name_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("unknown backend")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("nonexistent", wo).await {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent");
        }
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn backend_identity_appears_in_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "custom-id",
        IdentityBackend {
            name: "custom-id".into(),
            version: "3.5".into(),
            caps: CapabilityManifest::default(),
        },
    );

    let wo = WorkOrderBuilder::new("identity test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("custom-id", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.backend.id, "custom-id");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("3.5"));
}

#[tokio::test]
async fn backend_capabilities_match_manifest() {
    let rt = Runtime::with_default_backends();
    let backend = rt.backend("mock").expect("mock backend registered");
    let caps = backend.capabilities();

    assert!(caps.contains_key(&Capability::Streaming));
    assert!(caps.contains_key(&Capability::ToolRead));
    assert!(!caps.contains_key(&Capability::McpClient));
}

// ===========================================================================
// 3. Error propagation (5 tests)
// ===========================================================================

#[tokio::test]
async fn error_backend_failure_produces_error() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("fail test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;

    assert!(
        matches!(receipt, Err(RuntimeError::BackendFailed(_))),
        "expected BackendFailed, got {receipt:?}"
    );
}

#[tokio::test]
async fn error_capability_check_produces_clear_error() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let wo = WorkOrderBuilder::new("missing capability")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();

    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(msg.contains("mock"), "error should name the backend: {msg}");
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_unknown_backend_names_backend() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let err = match rt.run_streaming("ghost", wo).await {
        Err(e) => e,
        Ok(_) => panic!("expected error, got Ok"),
    };
    let msg = format!("{err}");
    assert!(msg.contains("ghost"), "error should contain backend name");
}

#[tokio::test]
async fn error_preflight_capability_check_api() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Native,
        }],
    };

    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[tokio::test]
async fn error_failing_backend_error_message_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("err msg test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;

    match receipt {
        Err(RuntimeError::BackendFailed(e)) => {
            let chain = format!("{e:#}");
            assert!(
                chain.contains("intentional failure"),
                "root cause should be preserved: {chain}"
            );
        }
        other => panic!("expected BackendFailed, got {other:?}"),
    }
}

// ===========================================================================
// 4. Receipt chain (5 tests)
// ===========================================================================

#[tokio::test]
async fn receipt_chain_multiple_runs_linkable() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let mut run_ids = Vec::new();
    for i in 0..3 {
        let receipt = run_mock_receipt(&rt, &format!("chain {i}")).await;
        run_ids.push(receipt.meta.run_id);
        store.save(&receipt).unwrap();
    }

    // All are loadable.
    for id in &run_ids {
        assert!(store.load(*id).is_ok());
    }

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 3);
}

#[tokio::test]
async fn receipt_hash_verification_after_pipeline() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock_receipt(&rt, "hash verify").await;

    let stored_hash = receipt.receipt_sha256.as_ref().unwrap();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(stored_hash, &recomputed);
}

#[tokio::test]
async fn receipt_metadata_includes_timing() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock_receipt(&rt, "timing test").await;

    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    // duration_ms is non-negative (u64 guarantees this but check value).
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn receipt_outcome_reflects_backend_result() {
    let rt = Runtime::with_default_backends();

    // Successful mock produces Complete.
    let receipt = run_mock_receipt(&rt, "outcome test").await;
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Failing backend produces error, not a receipt with Failed outcome.
    let mut rt2 = Runtime::new();
    rt2.register_backend("failing", FailingBackend);
    let wo = WorkOrderBuilder::new("fail outcome")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt2.run_streaming("failing", wo).await.unwrap();
    let (_, result) = drain_run(handle).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn receipt_chain_store_verify_individual() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let receipt = run_mock_receipt(&rt, "store verify").await;
    store.save(&receipt).unwrap();

    assert!(store.verify(receipt.meta.run_id).unwrap());
    let loaded = store.load(receipt.meta.run_id).unwrap();
    assert_eq!(loaded.outcome, Outcome::Complete);
    assert_eq!(loaded.receipt_sha256, receipt.receipt_sha256,);
}

// ===========================================================================
// 5. Cross-crate integration (5 tests)
// ===========================================================================

#[tokio::test]
async fn cross_policy_plus_runtime_denied_tool_check() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };

    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);

    // Pipeline still completes (policy is recorded, not actively enforced at
    // runtime in v0.1 — backends do their own enforcement).
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy cross-crate")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn cross_emulation_runtime_mock_full_flow() {
    // Configure emulation for ExtendedThinking.
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );

    // Build a runtime with emulation enabled and a backend that lacks
    // ExtendedThinking.
    let rt = Runtime::with_default_backends().with_emulation(emu_config.clone());

    assert!(rt.emulation_config().is_some());

    // MockBackend does NOT advertise ExtendedThinking, but emulation covers it.
    let wo = WorkOrderBuilder::new("emulation flow")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Emulation report should be embedded in usage_raw.
    let usage = receipt.usage_raw.as_object().unwrap();
    assert!(
        usage.contains_key("emulation"),
        "emulation report should be in usage_raw: {usage:?}"
    );
}

#[tokio::test]
async fn cross_dialect_detection_and_mapping() {
    // Detect dialect from a Claude-style message.
    let claude_msg = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hello"}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn"
    });

    let detector = DialectDetector::new();
    let result = detector.detect(&claude_msg).expect("should detect");
    assert_eq!(result.dialect, Dialect::Claude);
    assert!(result.confidence > 0.0);

    // Use mapping registry to validate feature support.
    let registry = known_rules();
    let rule = registry
        .lookup(
            Dialect::Claude,
            Dialect::OpenAi,
            abp_mapping::features::TOOL_USE,
        )
        .expect("rule should exist");
    assert!(rule.fidelity.is_lossless());

    // Build a matrix from the registry.
    let matrix = MappingMatrix::from_registry(&registry);
    assert!(matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Gemini));
}

#[tokio::test]
async fn cross_ir_lowering_roundtrip_metadata() {
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

    // Build an IR conversation.
    let conv = IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Summarize the file."))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Here is the summary.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc-1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({"path": "main.rs"}),
                },
            ],
        ));

    assert_eq!(conv.len(), 3);
    assert!(conv.system_message().is_some());
    assert_eq!(conv.tool_calls().len(), 1);

    // Serde roundtrip preserves structure.
    let json = serde_json::to_string(&conv).unwrap();
    let decoded: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.len(), conv.len());
    assert_eq!(decoded.tool_calls().len(), 1);

    // Verify metadata survives.
    let sys = decoded.system_message().unwrap();
    assert!(sys.text_content().contains("helpful assistant"));
}

#[tokio::test]
async fn cross_config_runtime_values_affect_behavior() {
    let rt = Runtime::with_default_backends();

    // Default mode is Mapped.
    let wo_default = WorkOrderBuilder::new("default mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo_default).await.unwrap();
    let (_, r1) = drain_run(handle).await;
    assert_eq!(r1.unwrap().mode, ExecutionMode::Mapped);

    // Passthrough mode via vendor config.
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo_pt = WorkOrderBuilder::new("passthrough mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let handle = rt.run_streaming("mock", wo_pt).await.unwrap();
    let (_, r2) = drain_run(handle).await;
    assert_eq!(r2.unwrap().mode, ExecutionMode::Passthrough);
}
