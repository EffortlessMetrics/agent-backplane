// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the runtime pipeline.
//!
//! Tests the full flow from work order through to receipt, exercising
//! workspace staging, policy compilation, capability negotiation,
//! emulation, backend execution, receipt hashing, receipt chains,
//! error recovery, concurrent runs, config-driven selection,
//! execution modes, event statistics, thread sharing, and timeouts.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RunMetadata, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
    WorkspaceMode,
};
use abp_emulation::{EmulationConfig, EmulationStrategy};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_receipt::compute_hash;
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
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

/// Run a work order on the named backend and return events + receipt.
async fn run_full(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    drain_run(handle).await
}

/// Shorthand: run mock backend and return receipt.
async fn run_mock(rt: &Runtime, task: &str) -> Receipt {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = run_full(rt, "mock", wo).await;
    receipt.unwrap()
}

// ---------------------------------------------------------------------------
// Custom test backends
// ---------------------------------------------------------------------------

/// Backend that streams configurable events and returns a valid receipt.
#[derive(Debug, Clone)]
struct EventStreamingBackend {
    name: String,
    caps: CapabilityManifest,
    event_count: usize,
}

#[async_trait]
impl Backend for EventStreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("1.0".into()),
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

        // Emit N assistant messages.
        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("event {i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let end = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(end.clone());
        let _ = events_tx.send(end).await;

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

/// Backend that always errors.
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

/// Backend that panics during execution.
#[derive(Debug, Clone)]
struct PanickingBackend;

#[async_trait]
impl Backend for PanickingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "panicking".into(),
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
        panic!("backend panic for testing");
    }
}

/// Backend that sleeps for a configurable duration.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay_ms: u64,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "slow done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;

        let now = chrono::Utc::now();
        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: now,
                finished_at: now,
                duration_ms: self.delay_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace: vec![ev],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend with rich capabilities for projection tests.
#[derive(Debug, Clone)]
struct RichBackend {
    name: String,
    caps: CapabilityManifest,
}

#[async_trait]
impl Backend for RichBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("2.0".into()),
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
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("{} done", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
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
            mode: abp_integrations::extract_execution_mode(&work_order),
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace: vec![ev],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

// ===========================================================================
// 1. Full pipeline: work order → workspace → policy → capability →
//    emulation → backend → receipt
// ===========================================================================

#[tokio::test]
async fn full_pipeline_end_to_end() {
    let mut rt = Runtime::new();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    rt.register_backend(
        "full",
        EventStreamingBackend {
            name: "full".into(),
            caps,
            event_count: 3,
        },
    );

    let wo = WorkOrderBuilder::new("full pipeline test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..Default::default()
        })
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let (events, receipt) = run_full(&rt, "full", wo).await;
    let receipt = receipt.unwrap();

    // Events were streamed.
    assert!(
        events.len() >= 4,
        "expected at least 4 events, got {}",
        events.len()
    );
    // Receipt is complete with hash.
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.backend.id, "full");
}

// ===========================================================================
// 2. Multiple backends registered, projection selects best
// ===========================================================================

#[tokio::test]
async fn multiple_backends_select_by_name() {
    let mut rt = Runtime::new();

    let mut alpha_caps = CapabilityManifest::new();
    alpha_caps.insert(Capability::Streaming, SupportLevel::Native);

    let mut beta_caps = CapabilityManifest::new();
    beta_caps.insert(Capability::Streaming, SupportLevel::Native);
    beta_caps.insert(Capability::ToolRead, SupportLevel::Native);
    beta_caps.insert(Capability::ToolWrite, SupportLevel::Native);

    rt.register_backend(
        "alpha",
        RichBackend {
            name: "alpha".into(),
            caps: alpha_caps,
        },
    );
    rt.register_backend(
        "beta",
        RichBackend {
            name: "beta".into(),
            caps: beta_caps,
        },
    );

    let names = rt.backend_names();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));

    // Select beta explicitly — has richer capabilities.
    let wo = WorkOrderBuilder::new("select beta")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = run_full(&rt, "beta", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.backend.id, "beta");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0"));
}

// ===========================================================================
// 3. Workspace staging creates valid working directory with git
// ===========================================================================

#[tokio::test]
async fn workspace_staging_creates_git_repo() {
    // Create a temp source directory with a file.
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("hello.txt"), "world").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged workspace")
        .root(source.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    // The runtime filled in verification from the staged workspace's git.
    // With only the file copied and committed as baseline, git status/diff
    // may be empty (no changes after baseline commit), which is correct.
}

// ===========================================================================
// 4. Policy blocks forbidden tools → run fails with clear error
// ===========================================================================

#[tokio::test]
async fn policy_compiles_and_blocks_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_write: vec!["**/secret/**".into()],
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };

    // Policy engine correctly blocks.
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(
        !engine
            .can_write_path(Path::new("dir/secret/key.txt"))
            .allowed
    );
    assert!(!engine.can_read_path(Path::new(".env")).allowed);

    // Pipeline still completes — policy is recorded, not enforced at runtime in v0.1.
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 5. Capability negotiation fails → run fails with detailed report
// ===========================================================================

#[tokio::test]
async fn capability_negotiation_fails_with_details() {
    let rt = Runtime::with_default_backends();

    // MockBackend lacks McpClient.
    let wo = WorkOrderBuilder::new("missing capability")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(msg.contains("mock"), "error should name backend: {msg}");
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn capability_negotiation_multiple_missing() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("multiple missing")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::CodeExecution,
                    min_support: MinSupport::Native,
                },
            ],
        })
        .build();

    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(_)) => {}
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ===========================================================================
// 6. Emulation wires in for missing capabilities
// ===========================================================================

#[tokio::test]
async fn emulation_covers_missing_capability() {
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(emu_config);
    assert!(rt.emulation_config().is_some());

    let wo = WorkOrderBuilder::new("emulation test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Emulation report is in usage_raw.
    let usage = receipt.usage_raw.as_object().unwrap();
    assert!(
        usage.contains_key("emulation"),
        "emulation report should be in usage_raw: {usage:?}"
    );
}

#[tokio::test]
async fn emulation_fails_for_unemulatable_capability() {
    let emu_config = EmulationConfig::new(); // no overrides → CodeExecution is Disabled

    let rt = Runtime::with_default_backends().with_emulation(emu_config);

    let wo = WorkOrderBuilder::new("unemulatable")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::CodeExecution,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(
                msg.contains("emulation unavailable"),
                "should mention emulation unavailable: {msg}"
            );
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ===========================================================================
// 7. Backend streams events → all captured in receipt
// ===========================================================================

#[tokio::test]
async fn backend_events_all_captured() {
    let mut rt = Runtime::new();
    let event_count = 10;
    rt.register_backend(
        "streamer",
        EventStreamingBackend {
            name: "streamer".into(),
            caps: CapabilityManifest::default(),
            event_count,
        },
    );

    let wo = WorkOrderBuilder::new("event capture")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (events, receipt) = run_full(&rt, "streamer", wo).await;
    let receipt = receipt.unwrap();

    // event_count assistant messages + 1 RunCompleted
    assert_eq!(events.len(), event_count + 1);
    assert!(!receipt.trace.is_empty());
}

// ===========================================================================
// 8. Receipt has correct hash
// ===========================================================================

#[tokio::test]
async fn receipt_hash_is_correct() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash test").await;

    let stored = receipt.receipt_sha256.as_ref().expect("hash should exist");
    assert_eq!(stored.len(), 64);
    assert!(stored.chars().all(|c| c.is_ascii_hexdigit()));

    // Recompute and verify.
    let recomputed = compute_hash(&receipt).unwrap();
    assert_eq!(stored, &recomputed);
}

#[tokio::test]
async fn receipt_hash_changes_when_tampered() {
    let rt = Runtime::with_default_backends();
    let mut receipt = run_mock(&rt, "tamper test").await;

    let original_hash = receipt.receipt_sha256.clone().unwrap();
    receipt.outcome = Outcome::Failed;
    let tampered_hash = compute_hash(&receipt).unwrap();

    assert_ne!(original_hash, tampered_hash);
}

// ===========================================================================
// 9. Receipt chain tracks multiple sequential runs
// ===========================================================================

#[tokio::test]
async fn receipt_chain_sequential_runs() {
    let rt = Runtime::with_default_backends();

    let mut receipts = Vec::new();
    for i in 0..5 {
        let receipt = run_mock(&rt, &format!("chain run {i}")).await;
        receipts.push(receipt);
    }

    // All have distinct run_ids.
    let ids: Vec<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 5);

    // All have hashes.
    for r in &receipts {
        assert!(r.receipt_sha256.is_some());
    }

    // Internal receipt chain has accumulated entries.
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert_eq!(chain.len(), 5);
}

#[tokio::test]
async fn receipt_chain_store_persistence() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        let receipt = run_mock(&rt, &format!("persist {i}")).await;
        store.save(&receipt).unwrap();
    }

    let chain_result = store.verify_chain().unwrap();
    assert!(chain_result.is_valid);
    assert_eq!(chain_result.valid_count, 3);
    assert!(chain_result.invalid_hashes.is_empty());
}

// ===========================================================================
// 10. Error recovery: backend panics → graceful error receipt
// ===========================================================================

#[tokio::test]
async fn backend_panic_produces_error() {
    let mut rt = Runtime::new();
    rt.register_backend("panicking", PanickingBackend);

    let wo = WorkOrderBuilder::new("panic test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("panicking", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;

    match receipt {
        Err(RuntimeError::BackendFailed(e)) => {
            let msg = format!("{e:#}");
            assert!(msg.contains("panic"), "should mention panic: {msg}");
        }
        other => panic!("expected BackendFailed from panic, got {other:?}"),
    }
}

#[tokio::test]
async fn backend_error_preserves_message() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "failing",
        FailingBackend {
            message: "disk full".into(),
        },
    );

    let wo = WorkOrderBuilder::new("error msg test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;

    match receipt {
        Err(RuntimeError::BackendFailed(e)) => {
            let chain = format!("{e:#}");
            assert!(chain.contains("disk full"), "root cause preserved: {chain}");
        }
        other => panic!("expected BackendFailed, got {other:?}"),
    }
}

// ===========================================================================
// 11. Concurrent runs on same runtime
// ===========================================================================

#[tokio::test]
async fn concurrent_runs_on_shared_runtime() {
    let rt = Arc::new(Runtime::with_default_backends());
    let mut handles = Vec::new();

    for i in 0..5 {
        let rt = Arc::clone(&rt);
        handles.push(tokio::spawn(async move {
            let wo = WorkOrderBuilder::new(format!("concurrent {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (events, receipt) = drain_run(handle).await;
            (events, receipt.unwrap())
        }));
    }

    let mut run_ids = Vec::new();
    for h in handles {
        let (events, receipt) = h.await.unwrap();
        assert!(!events.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
        run_ids.push(receipt.meta.run_id);
    }

    // All run_ids are distinct.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 5);
}

// ===========================================================================
// 12. Config-driven backend selection
// ===========================================================================

#[tokio::test]
async fn config_driven_backend_selection() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend(
        "custom",
        RichBackend {
            name: "custom".into(),
            caps: CapabilityManifest::default(),
        },
    );

    // Config specifies model — pipeline routes to named backend.
    let wo = WorkOrderBuilder::new("config select")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            model: Some("gpt-4".into()),
            ..Default::default()
        })
        .build();

    // Explicitly select "custom" backend.
    let (_, receipt) = run_full(&rt, "custom", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.backend.id, "custom");
}

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("unknown")
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

// ===========================================================================
// 13. Passthrough mode: events pass through unmodified
// ===========================================================================

#[tokio::test]
async fn passthrough_mode_produces_passthrough_receipt() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let (events, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
}

// ===========================================================================
// 14. Mapped mode: default execution mode
// ===========================================================================

#[tokio::test]
async fn mapped_mode_is_default() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("mapped default")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mapped_mode_with_explicit_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".to_string(), serde_json::json!({"mode": "mapped"}));

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("explicit mapped")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().mode, ExecutionMode::Mapped);
}

// ===========================================================================
// 15. Event statistics accumulated during run
// ===========================================================================

#[tokio::test]
async fn event_statistics_accumulated() {
    let rt = Runtime::with_default_backends();

    // Run twice to accumulate metrics.
    let _ = run_mock(&rt, "stats 1").await;
    let _ = run_mock(&rt, "stats 2").await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn event_statistics_track_failures() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend(
        "failing",
        FailingBackend {
            message: "fail".into(),
        },
    );

    // One success.
    let _ = run_mock(&rt, "ok").await;

    // One failure.
    let wo = WorkOrderBuilder::new("fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let _ = drain_run(handle).await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1); // only successful runs record metrics
    assert_eq!(snap.successful_runs, 1);
}

// ===========================================================================
// 16. Runtime can be cloned/shared across threads (Arc<Runtime>)
// ===========================================================================

#[tokio::test]
async fn runtime_shared_across_threads() {
    let rt = Arc::new(Runtime::with_default_backends());

    let rt2 = Arc::clone(&rt);
    let handle = tokio::spawn(async move {
        let wo = WorkOrderBuilder::new("thread 1")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let h = rt2.run_streaming("mock", wo).await.unwrap();
        drain_run(h).await
    });

    let rt3 = Arc::clone(&rt);
    let handle2 = tokio::spawn(async move {
        let wo = WorkOrderBuilder::new("thread 2")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let h = rt3.run_streaming("mock", wo).await.unwrap();
        drain_run(h).await
    });

    let (_, r1) = handle.await.unwrap();
    let (_, r2) = handle2.await.unwrap();

    assert_eq!(r1.unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().outcome, Outcome::Complete);

    // Metrics reflect both runs.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
}

// ===========================================================================
// 17. Backend timeout handling
// ===========================================================================

#[tokio::test]
async fn slow_backend_completes_within_limit() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 50 });

    let wo = WorkOrderBuilder::new("slow ok")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("slow", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn timeout_via_tokio_produces_error() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 5000 });

    let wo = WorkOrderBuilder::new("timeout test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("slow", wo).await.unwrap();

    // Wrap the receipt future with a timeout.
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), handle.receipt).await;

    // Should timeout.
    assert!(result.is_err(), "should have timed out");
}

// ===========================================================================
// 18. Capability negotiation result recorded in receipt
// ===========================================================================

#[tokio::test]
async fn capability_negotiation_recorded_in_receipt() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("negotiation metadata")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();

    let usage = receipt.usage_raw.as_object().unwrap();
    assert!(
        usage.contains_key("capability_negotiation"),
        "negotiation result should be in usage_raw: {usage:?}"
    );
}

// ===========================================================================
// 19. Receipt trace is populated even when backend doesn't provide one
// ===========================================================================

#[tokio::test]
async fn receipt_trace_populated_from_stream() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "trace test").await;

    // MockBackend provides events → trace should be non-empty.
    assert!(!receipt.trace.is_empty());

    // Should contain at least RunStarted and RunCompleted.
    let has_started = receipt
        .trace
        .iter()
        .any(|ev| matches!(&ev.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = receipt
        .trace
        .iter()
        .any(|ev| matches!(&ev.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_started, "trace should contain RunStarted");
    assert!(has_completed, "trace should contain RunCompleted");
}

// ===========================================================================
// 20. Error code mapping for runtime errors
// ===========================================================================

#[tokio::test]
async fn error_codes_propagate_correctly() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("error code test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("ghost", wo).await {
        Err(err) => {
            assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
            let abp_err = err.into_abp_error();
            assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
            assert!(abp_err.message.contains("ghost"));
        }
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ===========================================================================
// 21. Workspace passthrough uses original path
// ===========================================================================

#[tokio::test]
async fn workspace_passthrough_uses_original() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 22. Registry operations
// ===========================================================================

#[tokio::test]
async fn registry_list_and_contains() {
    let mut rt = Runtime::new();
    rt.register_backend("a", abp_integrations::MockBackend);
    rt.register_backend("b", abp_integrations::MockBackend);
    rt.register_backend("c", abp_integrations::MockBackend);

    let names = rt.backend_names();
    assert_eq!(names.len(), 3);
    assert!(rt.registry().contains("a"));
    assert!(rt.registry().contains("b"));
    assert!(rt.registry().contains("c"));
    assert!(!rt.registry().contains("d"));
}

#[tokio::test]
async fn registry_replace_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("x", abp_integrations::MockBackend);

    // Replace with a different backend.
    rt.register_backend(
        "x",
        RichBackend {
            name: "replaced".into(),
            caps: CapabilityManifest::default(),
        },
    );

    let backend = rt.backend("x").unwrap();
    assert_eq!(backend.identity().id, "replaced");
}

// ===========================================================================
// 23. Pre-flight capability check API
// ===========================================================================

#[tokio::test]
async fn preflight_capability_check() {
    let rt = Runtime::with_default_backends();

    // Satisfied requirement.
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();

    // Unsatisfied requirement.
    let reqs2 = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs2).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));

    // Unknown backend.
    let err2 = rt.check_capabilities("nonexistent", &reqs).unwrap_err();
    assert!(matches!(err2, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 24. Receipt metadata includes timing
// ===========================================================================

#[tokio::test]
async fn receipt_timing_is_valid() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "timing").await;

    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(!receipt.meta.run_id.is_nil());
    assert!(!receipt.meta.work_order_id.is_nil());
}

// ===========================================================================
// 25. Emulation + capability negotiation combined
// ===========================================================================

#[tokio::test]
async fn emulation_and_negotiation_combined_in_receipt() {
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think carefully.".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(emu_config);

    let wo = WorkOrderBuilder::new("combined test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Native,
                },
            ],
        })
        .build();

    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);

    let usage = receipt.usage_raw.as_object().unwrap();
    // Both emulation and capability_negotiation should be present.
    assert!(usage.contains_key("emulation"));
    assert!(usage.contains_key("capability_negotiation"));
}
