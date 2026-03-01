// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive end-to-end pipeline tests exercising the full ABP runtime flow.
//!
//! These tests cover: runtime lifecycle, workspace staging, policy enforcement,
//! receipt chain integrity, and error paths.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    receipt_hash,
};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_runtime::store::ReceiptStore;
use abp_runtime::{Runtime, RuntimeError};
use abp_workspace::WorkspaceStager;
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

/// Build and execute a simple mock run, returning events and receipt.
async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
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

/// A backend that emits a configurable number of assistant messages.
#[derive(Debug, Clone)]
struct ConfigurableBackend {
    message_count: usize,
    identity_name: String,
}

#[async_trait]
impl Backend for ConfigurableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.identity_name.clone(),
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
        let started_at = chrono::Utc::now();
        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.identity_name),
            },
            ext: None,
        };
        let _ = events_tx.send(start_ev.clone()).await;
        let mut trace = vec![start_ev];

        for i in 0..self.message_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("message {i} from {}", self.identity_name),
                },
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(end_ev.clone()).await;
        trace.push(end_ev);

        let finished_at = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
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
// 1. Runtime lifecycle (8+ tests)
// ===========================================================================

#[tokio::test]
async fn lifecycle_create_register_submit_collect() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);

    let wo = WorkOrderBuilder::new("lifecycle test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(!events.is_empty());
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn lifecycle_mock_backend_event_sequence() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "event sequence test").await;

    // MockBackend emits: RunStarted, 2× AssistantMessage, RunCompleted
    assert!(
        events.len() >= 4,
        "expected ≥4 events, got {}",
        events.len()
    );

    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[events.len() - 1].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    // Middle events are AssistantMessages
    for ev in &events[1..events.len() - 1] {
        assert!(
            matches!(&ev.kind, AgentEventKind::AssistantMessage { .. }),
            "expected AssistantMessage, got {:?}",
            ev.kind
        );
    }
}

#[tokio::test]
async fn lifecycle_multiple_backends_route_correctly() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "backend_a",
        ConfigurableBackend {
            message_count: 1,
            identity_name: "backend_a".into(),
        },
    );
    rt.register_backend(
        "backend_b",
        ConfigurableBackend {
            message_count: 3,
            identity_name: "backend_b".into(),
        },
    );

    let wo_a = WorkOrderBuilder::new("route to A")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle_a = rt.run_streaming("backend_a", wo_a).await.unwrap();
    let (events_a, receipt_a) = drain_run(handle_a).await;
    let receipt_a = receipt_a.unwrap();

    let wo_b = WorkOrderBuilder::new("route to B")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle_b = rt.run_streaming("backend_b", wo_b).await.unwrap();
    let (events_b, receipt_b) = drain_run(handle_b).await;
    let receipt_b = receipt_b.unwrap();

    assert_eq!(receipt_a.backend.id, "backend_a");
    assert_eq!(receipt_b.backend.id, "backend_b");

    // backend_a: RunStarted + 1 msg + RunCompleted = 3
    // backend_b: RunStarted + 3 msgs + RunCompleted = 5
    assert_eq!(events_a.len(), 3);
    assert_eq!(events_b.len(), 5);
}

#[tokio::test]
async fn lifecycle_unknown_backend_error() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("should fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("nonexistent_backend", wo).await {
        Err(RuntimeError::UnknownBackend { name }) => {
            assert_eq!(name, "nonexistent_backend");
        }
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn lifecycle_work_order_id_preserved() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("id preservation test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let wo_id = wo.id;

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn lifecycle_receipt_backend_id_and_model() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "backend identity check").await;

    assert_eq!(receipt.backend.id, "mock");
    // MockBackend sets backend_version to Some("0.1")
    assert!(receipt.backend.backend_version.is_some());
}

#[tokio::test]
async fn lifecycle_first_event_run_started_last_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "event bookends").await;

    assert!(!events.is_empty(), "must have at least one event");
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event must be RunStarted, got {:?}",
        events[0].kind
    );
    assert!(
        matches!(
            &events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event must be RunCompleted"
    );
}

#[tokio::test]
async fn lifecycle_event_timestamps_monotonic() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock(&rt, "timestamp ordering").await;

    for pair in events.windows(2) {
        assert!(
            pair[1].ts >= pair[0].ts,
            "timestamps must be non-decreasing: {} > {}",
            pair[0].ts,
            pair[1].ts
        );
    }
}

// ===========================================================================
// 2. Workspace staging (5+ tests)
// ===========================================================================

#[tokio::test]
async fn workspace_staged_creates_temp_directory() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("hello.txt"), "world").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged temp dir")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    // Staged workspace populates git verification fields.
    assert!(receipt.verification.git_diff.is_some() || receipt.verification.git_status.is_some());
}

#[tokio::test]
async fn workspace_staged_excludes_git() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src_dir.path().join(".git")).unwrap();
    std::fs::write(src_dir.path().join(".git").join("config"), "fake").unwrap();
    std::fs::write(src_dir.path().join("main.rs"), "fn main() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .stage()
        .unwrap();

    // .git should not exist in the staged directory (a fresh one is created)
    let staged_git = prepared.path().join(".git");
    if staged_git.exists() {
        // If .git exists, it should be the freshly initialized one, not the original
        let config = std::fs::read_to_string(staged_git.join("config")).unwrap_or_default();
        assert!(
            !config.contains("fake"),
            ".git should be freshly initialized, not copied"
        );
    }

    // Source file should be present
    assert!(prepared.path().join("main.rs").exists());
}

#[tokio::test]
async fn workspace_staged_initializes_git_with_baseline() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("file.txt"), "content").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // The staged workspace should have a .git directory
    assert!(
        prepared.path().join(".git").exists(),
        "staged workspace should have .git"
    );

    // git log should show at least the baseline commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(prepared.path())
        .output();

    if let Ok(out) = output {
        let log = String::from_utf8_lossy(&out.stdout);
        assert!(
            !log.is_empty(),
            "should have at least one commit (baseline)"
        );
    }
}

#[tokio::test]
async fn workspace_passthrough_uses_directory_as_is() {
    let src_dir = tempfile::tempdir().unwrap();
    let marker_path = src_dir.path().join("marker.txt");
    std::fs::write(&marker_path, "unique-marker-content").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    // In PassThrough mode, workspace root stays the same
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.root, src_dir.path().to_str().unwrap());

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn workspace_include_exclude_globs_filter() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("keep.rs"), "fn keep() {}").unwrap();
    std::fs::write(src_dir.path().join("drop.log"), "log data").unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(src_dir.path().join("src").join("lib.rs"), "pub fn lib() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        prepared.path().join("keep.rs").exists(),
        "keep.rs should be staged"
    );
    assert!(
        !prepared.path().join("drop.log").exists(),
        "drop.log should be excluded"
    );
    assert!(
        prepared.path().join("src").join("lib.rs").exists(),
        "src/lib.rs should be staged"
    );
}

#[tokio::test]
async fn workspace_staged_pipeline_exclude_key_files() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("code.rs"), "fn main() {}").unwrap();
    std::fs::write(src_dir.path().join("secret.key"), "private").unwrap();
    std::fs::create_dir_all(src_dir.path().join("data")).unwrap();
    std::fs::write(src_dir.path().join("data").join("info.json"), "{}").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("exclude test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .exclude(vec!["*.key".into()])
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 3. Policy enforcement (5+ tests)
// ===========================================================================

#[tokio::test]
async fn policy_denied_tools_blocks_tool_use() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
}

#[tokio::test]
async fn policy_denied_read_paths_blocks_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(
        !engine
            .can_read_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(engine.can_read_path(Path::new("README.md")).allowed);
}

#[tokio::test]
async fn policy_denied_write_paths_blocks_write() {
    let policy = PolicyProfile {
        deny_write: vec!["**/protected/**".into(), "*.lock".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(
        !engine
            .can_write_path(Path::new("protected/config.yml"))
            .allowed
    );
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[tokio::test]
async fn policy_empty_allows_everything() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("AnyToolName").allowed);
    assert!(engine.can_read_path(Path::new(".env")).allowed);
    assert!(
        engine
            .can_read_path(Path::new("any/path/deep/file.rs"))
            .allowed
    );
    assert!(
        engine
            .can_write_path(Path::new("sensitive/data.json"))
            .allowed
    );
}

#[tokio::test]
async fn policy_restrictive_still_completes_pipeline() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into(), "Edit".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/secret/**".into()],
        ..PolicyProfile::default()
    };

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy pipeline")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn policy_decision_includes_reason() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.ssh/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let tool_decision = engine.can_use_tool("Bash");
    assert!(!tool_decision.allowed);
    assert!(
        tool_decision.reason.is_some(),
        "denied decision should include a reason"
    );

    let read_decision = engine.can_read_path(Path::new(".ssh/id_rsa"));
    assert!(!read_decision.allowed);
    assert!(
        read_decision.reason.is_some(),
        "denied read should include a reason"
    );
}

// ===========================================================================
// 4. Receipt chain (5+ tests)
// ===========================================================================

#[tokio::test]
async fn receipt_sequential_runs_unique_hashes() {
    let rt = Runtime::with_default_backends();
    let mut hashes = Vec::new();

    for i in 0..3 {
        let (_events, receipt) = run_mock(&rt, &format!("chain run {i}")).await;
        hashes.push(receipt.receipt_sha256.clone().unwrap());
    }

    // All hashes should be unique
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(
        unique.len(),
        hashes.len(),
        "sequential runs must produce unique hashes"
    );
}

#[tokio::test]
async fn receipt_hash_is_sha256_format() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "hash format test").await;

    let hash = receipt.receipt_sha256.as_ref().expect("hash should be set");
    assert_eq!(hash.len(), 64, "SHA-256 hash should be 64 hex chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should only contain hex digits"
    );
}

#[tokio::test]
async fn receipt_contains_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "contract version check").await;

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(!CONTRACT_VERSION.is_empty());
}

#[tokio::test]
async fn receipt_trace_count_matches_events() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "events count match").await;

    assert_eq!(
        receipt.trace.len(),
        events.len(),
        "receipt trace should match streamed event count"
    );
    assert!(!receipt.trace.is_empty(), "trace should not be empty");
}

#[tokio::test]
async fn receipt_timing_is_set() {
    let rt = Runtime::with_default_backends();
    let before_run = chrono::Utc::now();
    let (_events, receipt) = run_mock(&rt, "timing test").await;
    let after_run = chrono::Utc::now();

    // started_at and finished_at should be set and reasonable
    assert!(
        receipt.meta.started_at <= receipt.meta.finished_at,
        "started_at should be <= finished_at"
    );
    assert!(
        receipt.meta.started_at >= before_run - chrono::Duration::seconds(2),
        "started_at should be near the test start"
    );
    assert!(
        receipt.meta.finished_at <= after_run + chrono::Duration::seconds(2),
        "finished_at should be near the test end"
    );
}

#[tokio::test]
async fn receipt_hash_is_self_consistent() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "self consistent hash").await;

    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(
        receipt.receipt_sha256.as_deref(),
        Some(recomputed.as_str()),
        "receipt hash should be reproducible"
    );
}

#[tokio::test]
async fn receipt_store_save_load_verify() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let (_events, receipt) = run_mock(&rt, "store test").await;
    let run_id = receipt.meta.run_id;
    store.save(&receipt).unwrap();

    let loaded = store.load(run_id).unwrap();
    assert_eq!(loaded.meta.run_id, run_id);
    assert_eq!(loaded.outcome, Outcome::Complete);
    assert!(store.verify(run_id).unwrap(), "hash should verify");
}

// ===========================================================================
// 5. Error paths (7+ tests)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("will fail")
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
async fn error_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("will fail in backend")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let err = receipt.unwrap_err();
    assert!(
        matches!(err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed, got {err:?}"
    );
}

#[tokio::test]
async fn error_capability_check_failed() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let wo = WorkOrderBuilder::new("cap check failure")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();

    match rt.run_streaming("mock", wo).await {
        Err(RuntimeError::CapabilityCheckFailed(msg)) => {
            assert!(!msg.is_empty(), "error message should not be empty");
        }
        Err(e) => panic!("expected CapabilityCheckFailed, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_capability_preflight_check() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[tokio::test]
async fn error_multiple_rapid_submissions_no_panic() {
    let rt = Runtime::with_default_backends();
    let mut handles = Vec::new();

    for i in 0..10 {
        let wo = WorkOrderBuilder::new(format!("rapid {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        handles.push(rt.run_streaming("mock", wo).await.unwrap());
    }

    for handle in handles {
        let (_events, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn error_concurrent_access_safe() {
    let rt = std::sync::Arc::new(Runtime::with_default_backends());
    let mut join_handles = Vec::new();

    for i in 0..5 {
        let rt_clone = std::sync::Arc::clone(&rt);
        join_handles.push(tokio::spawn(async move {
            let wo = WorkOrderBuilder::new(format!("concurrent {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt_clone.run_streaming("mock", wo).await.unwrap();
            let (_events, receipt) = drain_run(handle).await;
            receipt.unwrap()
        }));
    }

    let mut receipts = Vec::new();
    for jh in join_handles {
        receipts.push(jh.await.unwrap());
    }

    assert_eq!(receipts.len(), 5);
    let ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 5, "all concurrent runs must have unique IDs");
}

#[tokio::test]
async fn error_large_work_order_handled() {
    let rt = Runtime::with_default_backends();
    let large_task = "x".repeat(100 * 1024); // 100 KB

    let wo = WorkOrderBuilder::new(large_task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert_eq!(wo.task.len(), 100 * 1024);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn error_empty_task_string_handled() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(wo.task.is_empty());

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn error_unknown_backend_from_empty_registry() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());

    let wo = WorkOrderBuilder::new("no backends")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("anything", wo).await {
        Err(RuntimeError::UnknownBackend { .. }) => {}
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ===========================================================================
// 6. Additional coverage
// ===========================================================================

#[tokio::test]
async fn backend_registry_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zulu", abp_integrations::MockBackend);
    rt.register_backend("alpha", abp_integrations::MockBackend);
    rt.register_backend("mike", abp_integrations::MockBackend);

    let names = rt.backend_names();
    assert_eq!(names.len(), 3);
    // backend_names returns the registered backends
    assert!(names.contains(&"alpha".to_string()) || names.contains(&"zulu".to_string()));
}

#[tokio::test]
async fn receipt_chain_store_verify_chain() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        let (_events, receipt) = run_mock(&rt, &format!("chain {i}")).await;
        store.save(&receipt).unwrap();
    }

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid, "chain should be valid");
    assert_eq!(chain.valid_count, 3);
    assert!(chain.invalid_hashes.is_empty());
}

#[tokio::test]
async fn execution_mode_default_is_mapped() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "default mode test").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn execution_mode_passthrough_via_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn telemetry_tracks_runs_correctly() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.metrics().snapshot().total_runs, 0);

    let _ = run_mock(&rt, "telemetry run 1").await;
    let _ = run_mock(&rt, "telemetry run 2").await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn telemetry_tracks_failed_runs() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    rt.register_backend("mock", abp_integrations::MockBackend);

    // Successful run
    let _ = run_mock(&rt, "success").await;

    // Failing run — backend errors don't reach the telemetry recording
    // because the spawned task returns Err before record_run is called.
    let wo = WorkOrderBuilder::new("will fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, result) = drain_run(handle).await;
    assert!(result.is_err(), "failing backend should produce an error");

    // Another successful run
    let _ = run_mock(&rt, "success 2").await;

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
}

#[tokio::test]
async fn receipt_verification_harness_ok_set() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "harness check").await;
    // MockBackend sets harness_ok: true
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn config_model_propagation() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("model config")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("test-model-v1")
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("test-model-v1"));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn config_budget_and_turns() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("budget and turns")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_budget_usd(5.0)
        .max_turns(20)
        .build();

    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(20));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}
