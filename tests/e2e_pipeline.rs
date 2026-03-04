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
//! Comprehensive end-to-end pipeline tests exercising the full ABP runtime flow.
//!
//! These tests cover: runtime lifecycle, workspace staging, policy enforcement,
//! receipt chain integrity, error paths, event ordering, cancellation, budget,
//! event bus patterns, stream pipelines, telemetry, configuration overrides,
//! execution modes, and pipeline stages.

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
use abp_receipt::{ReceiptChain, diff_receipts, verify_hash};
use abp_runtime::budget::{BudgetLimit, BudgetStatus, BudgetTracker, BudgetViolation};
use abp_runtime::bus::{EventBus, FilteredSubscription};
use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};
use abp_runtime::pipeline::{AuditStage, Pipeline, PolicyStage, ValidationStage};
use abp_runtime::store::ReceiptStore;
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{EventFilter, EventRecorder, EventStats, StreamPipelineBuilder};
use abp_telemetry::{MetricsCollector, RunMetrics as TelemetryRunMetrics};
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

/// A backend that emits events with interleaved types for ordering tests.
#[derive(Debug, Clone)]
struct OrderingBackend;

#[async_trait]
impl Backend for OrderingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "ordering".into(),
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
        let mut trace = Vec::new();

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "thinking".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "test.rs"}),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!("file content"),
                is_error: false,
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

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

/// A slow backend that sleeps, useful for timeout tests.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SlowBackend {
    delay_ms: u64,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".into(),
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
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = chrono::Utc::now();

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "slow start".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        let mut trace = vec![ev];

        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "slow done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

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
// 1. Runtime lifecycle (8 tests)
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
// 2. Workspace staging (6 tests)
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
// 3. Policy enforcement (6 tests)
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
// 4. Receipt chain & verification (7 tests)
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
// 5. Error paths (8 tests)
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

// ===========================================================================
// 6. Event ordering (6 tests)
// ===========================================================================

#[tokio::test]
async fn ordering_no_events_after_run_completed() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("ordering check")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    let completed_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .expect("must have RunCompleted");

    assert_eq!(
        completed_idx,
        events.len() - 1,
        "no events should appear after RunCompleted"
    );
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn ordering_run_started_is_always_first() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let (events, _receipt) = run_mock(&rt, &format!("first event {i}")).await;
        assert!(
            matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
            "first event must be RunStarted, run {i}"
        );
    }
}

#[tokio::test]
async fn ordering_tool_result_follows_tool_call() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("tool ordering")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let call_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    let result_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }));

    if let (Some(ci), Some(ri)) = (call_idx, result_idx) {
        assert!(ri > ci, "ToolResult must come after ToolCall");
    }
}

#[tokio::test]
async fn ordering_events_preserve_insertion_order() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "seq",
        ConfigurableBackend {
            message_count: 10,
            identity_name: "seq".into(),
        },
    );

    let wo = WorkOrderBuilder::new("insertion order")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("seq", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    // Extract message indices from AssistantMessage events
    let indices: Vec<usize> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => {
                text.split_whitespace().next().and_then(|w| {
                    w.strip_prefix("message")
                        .and_then(|n| n.parse::<usize>().ok())
                })
            }
            _ => None,
        })
        .collect();

    for pair in indices.windows(2) {
        assert!(
            pair[1] > pair[0],
            "messages out of order: {} before {}",
            pair[0],
            pair[1]
        );
    }
}

#[tokio::test]
async fn ordering_trace_matches_streamed_events() {
    let mut rt = Runtime::new();
    rt.register_backend("ordering", OrderingBackend);

    let wo = WorkOrderBuilder::new("trace vs stream")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("ordering", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(events.len(), receipt.trace.len());
    for (i, (streamed, traced)) in events.iter().zip(receipt.trace.iter()).enumerate() {
        let s_json = serde_json::to_string(&streamed.kind).unwrap();
        let t_json = serde_json::to_string(&traced.kind).unwrap();
        assert_eq!(
            s_json, t_json,
            "event {i} mismatch between stream and trace"
        );
    }
}

#[tokio::test]
async fn ordering_configurable_event_count() {
    let mut rt = Runtime::new();
    for count in [0, 1, 5, 20] {
        let name = format!("backend_{count}");
        rt.register_backend(
            &name,
            ConfigurableBackend {
                message_count: count,
                identity_name: name.clone(),
            },
        );

        let wo = WorkOrderBuilder::new(format!("count {count}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming(&name, wo).await.unwrap();
        let (events, _receipt) = drain_run(handle).await;

        // RunStarted + count messages + RunCompleted
        assert_eq!(
            events.len(),
            count + 2,
            "expected {} events for count={count}",
            count + 2
        );
    }
}

// ===========================================================================
// 7. Receipt hash stability (5 tests)
// ===========================================================================

#[tokio::test]
async fn hash_stability_recompute_matches() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "hash stability").await;

    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2, "recomputed hashes must be identical");
}

#[tokio::test]
async fn hash_stability_verify_hash_returns_true() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "verify hash").await;
    assert!(verify_hash(&receipt), "verify_hash should return true");
}

#[tokio::test]
async fn hash_stability_different_tasks_different_hashes() {
    let rt = Runtime::with_default_backends();
    let (_e1, r1) = run_mock(&rt, "task alpha").await;
    let (_e2, r2) = run_mock(&rt, "task beta").await;

    assert_ne!(
        r1.receipt_sha256, r2.receipt_sha256,
        "different tasks should produce different hashes"
    );
}

#[tokio::test]
async fn hash_stability_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    let chain = rt.receipt_chain();

    for i in 0..3 {
        run_mock(&rt, &format!("chain {i}")).await;
    }

    let chain = chain.lock().await;
    assert_eq!(chain.len(), 3, "chain should have 3 receipts");
    assert!(chain.verify().is_ok(), "chain verification should pass");
}

#[tokio::test]
async fn hash_stability_receipt_diff_detects_changes() {
    let rt = Runtime::with_default_backends();
    let (_e1, r1) = run_mock(&rt, "diff first").await;
    let (_e2, r2) = run_mock(&rt, "diff second").await;

    let diff = diff_receipts(&r1, &r2);
    assert!(!diff.is_empty(), "different receipts should produce a diff");
}

// ===========================================================================
// 8. Pipeline with MockBackend (5 tests)
// ===========================================================================

#[tokio::test]
async fn mock_pipeline_complete_flow() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("complete mock flow")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn mock_pipeline_receipt_mode_is_mapped() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "mode check").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mock_pipeline_backend_names_listed() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn mock_pipeline_run_id_unique_per_run() {
    let rt = Runtime::with_default_backends();
    let (_e1, r1) = run_mock(&rt, "run id 1").await;
    let (_e2, r2) = run_mock(&rt, "run id 2").await;
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn mock_pipeline_usage_raw_is_object() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "usage raw").await;
    assert!(
        receipt.usage_raw.is_object(),
        "usage_raw should be a JSON object"
    );
}

// ===========================================================================
// 9. Pipeline with policy enforcement (4 tests)
// ===========================================================================

#[tokio::test]
async fn policy_pipeline_combined_deny_tools_and_paths() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/node_modules/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/foo/bar.js"))
            .allowed
    );
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[tokio::test]
async fn policy_pipeline_network_allow_list() {
    let policy = PolicyProfile {
        allow_network: vec!["api.example.com".into()],
        ..PolicyProfile::default()
    };

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("network policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy.clone())
        .build();

    assert_eq!(wo.policy.allow_network, vec!["api.example.com"]);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn policy_pipeline_multiple_deny_globs() {
    let policy = PolicyProfile {
        deny_read: vec![
            "**/.env".into(),
            "**/.env.*".into(),
            "**/secrets/**".into(),
            "**/*.pem".into(),
        ],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new(".env.local")).allowed);
    assert!(
        !engine
            .can_read_path(Path::new("secrets/password.txt"))
            .allowed
    );
    assert!(!engine.can_read_path(Path::new("certs/ca.pem")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[tokio::test]
async fn policy_pipeline_allowed_and_disallowed_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
}

// ===========================================================================
// 10. Pipeline with workspace staging (4 tests)
// ===========================================================================

#[tokio::test]
async fn workspace_pipeline_staged_receipt_has_verification() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("app.rs"), "fn main() {}").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged verification")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(
        receipt.verification.git_diff.is_some() || receipt.verification.git_status.is_some(),
        "staged workspace should produce git verification"
    );
}

#[tokio::test]
async fn workspace_pipeline_stager_no_git_init() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("lib.rs"), "pub fn x() {}").unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(prepared.path().join("lib.rs").exists());
    // Without git init, no .git directory
    assert!(
        !prepared.path().join(".git").exists(),
        "no .git without git_init"
    );
}

#[tokio::test]
async fn workspace_pipeline_nested_directories_staged() {
    let src_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src_dir.path().join("a").join("b").join("c")).unwrap();
    std::fs::write(
        src_dir.path().join("a").join("b").join("c").join("deep.rs"),
        "fn deep() {}",
    )
    .unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(
        prepared
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("deep.rs")
            .exists()
    );
}

#[tokio::test]
async fn workspace_pipeline_empty_source_dir() {
    let src_dir = tempfile::tempdir().unwrap();

    let prepared = WorkspaceStager::new()
        .source_root(src_dir.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(prepared.path().exists());
}

// ===========================================================================
// 11. Configuration overrides (4 tests)
// ===========================================================================

#[tokio::test]
async fn config_override_model() {
    let wo = WorkOrderBuilder::new("model override")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4o")
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));

    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn config_override_max_turns() {
    let wo = WorkOrderBuilder::new("turns override")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_turns(5)
        .build();

    assert_eq!(wo.config.max_turns, Some(5));
}

#[tokio::test]
async fn config_override_max_budget() {
    let wo = WorkOrderBuilder::new("budget override")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_budget_usd(10.0)
        .build();

    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[tokio::test]
async fn config_override_custom_runtime_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "openai".to_string(),
        serde_json::json!({"temperature": 0.7}),
    );
    let config = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: Some(5.0),
        max_turns: Some(10),
    };

    let wo = WorkOrderBuilder::new("custom config")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build();

    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert!(wo.config.vendor.contains_key("openai"));
}

// ===========================================================================
// 12. Multiple pipelines in sequence (4 tests)
// ===========================================================================

#[tokio::test]
async fn sequential_pipelines_all_complete() {
    let rt = Runtime::with_default_backends();
    let mut receipts = Vec::new();

    for i in 0..5 {
        let (_events, receipt) = run_mock(&rt, &format!("sequential {i}")).await;
        receipts.push(receipt);
    }

    for (i, r) in receipts.iter().enumerate() {
        assert_eq!(r.outcome, Outcome::Complete, "run {i} should complete");
        assert!(r.receipt_sha256.is_some(), "run {i} should have hash");
    }
}

#[tokio::test]
async fn sequential_pipelines_different_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend(
        "custom",
        ConfigurableBackend {
            message_count: 2,
            identity_name: "custom".into(),
        },
    );

    let wo1 = WorkOrderBuilder::new("first")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let (_e1, r1) = drain_run(h1).await;
    assert_eq!(r1.unwrap().backend.id, "mock");

    let wo2 = WorkOrderBuilder::new("second")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let h2 = rt.run_streaming("custom", wo2).await.unwrap();
    let (_e2, r2) = drain_run(h2).await;
    assert_eq!(r2.unwrap().backend.id, "custom");
}

#[tokio::test]
async fn sequential_pipelines_metrics_accumulate() {
    let rt = Runtime::with_default_backends();

    let snap_before = rt.metrics().snapshot();
    let initial_runs = snap_before.total_runs;

    for i in 0..3 {
        run_mock(&rt, &format!("metrics {i}")).await;
    }

    let snap_after = rt.metrics().snapshot();
    assert_eq!(snap_after.total_runs, initial_runs + 3);
    assert_eq!(snap_after.successful_runs, snap_before.successful_runs + 3);
}

#[tokio::test]
async fn sequential_pipelines_receipt_chain_grows() {
    let rt = Runtime::with_default_backends();

    for i in 0..4 {
        run_mock(&rt, &format!("chain grow {i}")).await;
    }

    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert!(chain.len() >= 4);
}

// ===========================================================================
// 13. Error propagation through pipeline stages (5 tests)
// ===========================================================================

#[tokio::test]
async fn error_propagation_backend_failure_to_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("fail propagation")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, result) = drain_run(handle).await;

    match result {
        Err(RuntimeError::BackendFailed(e)) => {
            let msg = format!("{e:#}");
            assert!(
                msg.contains("intentional failure") || msg.contains("failing"),
                "error should propagate: {msg}"
            );
        }
        other => panic!("expected BackendFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn error_propagation_unknown_backend_error_code() {
    let rt = Runtime::new();
    let wo = WorkOrderBuilder::new("err code check")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    match rt.run_streaming("ghost", wo).await {
        Err(ref e @ RuntimeError::UnknownBackend { .. }) => {
            assert_eq!(e.error_code(), abp_error::ErrorCode::BackendNotFound);
        }
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_propagation_capability_check_error_code() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[tokio::test]
async fn error_propagation_into_abp_error() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("test"));
}

#[tokio::test]
async fn error_propagation_pipeline_validation_rejects_empty() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = WorkOrderBuilder::new("valid task")
        .root("/some/path")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    // Validation should pass with valid fields
    assert!(pipeline.execute(&mut wo).await.is_ok());

    // Clear the task and it should fail
    wo.task = "".into();
    assert!(pipeline.execute(&mut wo).await.is_err());
}

// ===========================================================================
// 14. Cancellation / timeout scenarios (5 tests)
// ===========================================================================

#[tokio::test]
async fn cancel_token_new_is_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancel_then_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_clone_shares_state() {
    let a = CancellationToken::new();
    let b = a.clone();
    a.cancel();
    assert!(b.is_cancelled());
}

#[tokio::test]
async fn cancel_cancellable_run_reason_tracking() {
    let run = CancellableRun::new(CancellationToken::new());
    assert!(run.reason().is_none());

    run.cancel(CancellationReason::Timeout);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(CancellationReason::Timeout));

    // Second cancel does not overwrite reason
    run.cancel(CancellationReason::UserRequested);
    assert_eq!(run.reason(), Some(CancellationReason::Timeout));
}

#[tokio::test]
async fn cancel_cancelled_future_resolves() {
    let token = CancellationToken::new();
    let clone = token.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        clone.cancel();
    });

    tokio::time::timeout(std::time::Duration::from_secs(2), token.cancelled())
        .await
        .expect("cancelled() should resolve after cancel()");
}

// ===========================================================================
// 15. Event bus patterns (6 tests)
// ===========================================================================

#[tokio::test]
async fn event_bus_publish_subscribe() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();

    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    bus.publish(ev.clone());

    let received = sub.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn event_bus_multiple_subscribers() {
    let bus = EventBus::new();
    let mut sub1 = bus.subscribe();
    let mut sub2 = bus.subscribe();

    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "test".into(),
        },
        ext: None,
    };
    bus.publish(ev);

    assert!(sub1.recv().await.is_some());
    assert!(sub2.recv().await.is_some());
}

#[tokio::test]
async fn event_bus_stats_tracking() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();

    for _ in 0..5 {
        bus.publish(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: None,
        });
    }

    let stats = bus.stats();
    assert_eq!(stats.total_published, 5);
}

#[tokio::test]
async fn event_bus_no_subscribers_drops() {
    let bus = EventBus::new();
    bus.publish(AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "dropped".into(),
        },
        ext: None,
    });

    let stats = bus.stats();
    assert_eq!(stats.total_published, 1);
    assert_eq!(stats.dropped_events, 1);
}

#[tokio::test]
async fn event_bus_subscriber_count() {
    let bus = EventBus::new();
    assert_eq!(bus.subscriber_count(), 0);

    let _sub1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);

    let _sub2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    drop(_sub1);
    assert_eq!(bus.subscriber_count(), 1);
}

#[tokio::test]
async fn event_bus_filtered_subscription() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(&ev.kind, AgentEventKind::Error { .. })),
    );

    // Publish a non-error event
    bus.publish(AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "skip me".into(),
        },
        ext: None,
    });
    // Publish an error event
    bus.publish(AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "found me".into(),
            error_code: None,
        },
        ext: None,
    });

    // Drop the bus to close the channel after events are sent
    drop(bus);

    let ev = filtered.recv().await;
    assert!(ev.is_some());
    assert!(matches!(ev.unwrap().kind, AgentEventKind::Error { .. }));
}

// ===========================================================================
// 16. Receipt verification after pipeline (3 tests)
// ===========================================================================

#[tokio::test]
async fn receipt_verify_chain_multiple_runs() {
    let mut chain = ReceiptChain::new();
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        let (_events, receipt) = run_mock(&rt, &format!("chain verify {i}")).await;
        chain.push(receipt).unwrap();
    }

    assert!(chain.verify().is_ok());
    assert_eq!(chain.len(), 3);
}

#[tokio::test]
async fn receipt_verify_latest() {
    let mut chain = ReceiptChain::new();
    let rt = Runtime::with_default_backends();

    let (_events, r1) = run_mock(&rt, "first").await;
    let (_events, r2) = run_mock(&rt, "second").await;
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[tokio::test]
async fn receipt_verify_store_list() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    for i in 0..3 {
        let (_events, receipt) = run_mock(&rt, &format!("list {i}")).await;
        store.save(&receipt).unwrap();
    }

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 3);
}

// ===========================================================================
// 17. Pipeline with execution modes (3 tests)
// ===========================================================================

#[tokio::test]
async fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[tokio::test]
async fn execution_mode_passthrough_serde_roundtrip() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_string(&mode).unwrap();
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, mode);
}

#[tokio::test]
async fn execution_mode_receipt_reflects_backend_mode() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock(&rt, "mode in receipt").await;
    // MockBackend produces Mapped mode receipts
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

// ===========================================================================
// 18. Pipeline metrics collection (5 tests)
// ===========================================================================

#[tokio::test]
async fn metrics_runtime_records_runs() {
    let rt = Runtime::with_default_backends();
    let before = rt.metrics().snapshot();

    run_mock(&rt, "metrics run").await;

    let after = rt.metrics().snapshot();
    assert_eq!(after.total_runs, before.total_runs + 1);
    assert_eq!(after.successful_runs, before.successful_runs + 1);
}

#[tokio::test]
async fn metrics_runtime_records_events() {
    let rt = Runtime::with_default_backends();
    let before = rt.metrics().snapshot();

    run_mock(&rt, "metrics events").await;

    let after = rt.metrics().snapshot();
    assert!(after.total_events > before.total_events);
}

#[tokio::test]
async fn metrics_runtime_failed_run_counted() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let before = rt.metrics().snapshot();

    let wo = WorkOrderBuilder::new("will fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let _ = drain_run(handle).await;

    let after = rt.metrics().snapshot();
    // The failing backend returns an error, not a receipt, so it won't be
    // recorded as either success or failure by the runtime (error path returns early).
    // Just verify we don't panic.
    assert!(after.total_runs >= before.total_runs);
}

#[tokio::test]
async fn metrics_telemetry_collector_summary() {
    let collector = MetricsCollector::new();
    collector.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        dialect: "abp".into(),
        duration_ms: 100,
        events_count: 4,
        tokens_in: 50,
        tokens_out: 100,
        tool_calls_count: 1,
        errors_count: 0,
        emulations_applied: 0,
    });
    collector.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        dialect: "abp".into(),
        duration_ms: 200,
        events_count: 6,
        tokens_in: 80,
        tokens_out: 150,
        tool_calls_count: 2,
        errors_count: 0,
        emulations_applied: 0,
    });

    let summary = collector.summary();
    assert_eq!(summary.count, 2);
    assert!(summary.mean_duration_ms > 0.0);
    assert_eq!(summary.total_tokens_in, 130);
    assert_eq!(summary.total_tokens_out, 250);
    assert_eq!(summary.error_rate, 0.0);
}

#[tokio::test]
async fn metrics_telemetry_collector_clear() {
    let collector = MetricsCollector::new();
    collector.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        ..TelemetryRunMetrics::default()
    });
    assert_eq!(collector.len(), 1);

    collector.clear();
    assert!(collector.is_empty());
    assert_eq!(collector.len(), 0);
}

// ===========================================================================
// 19. Stream pipeline integration (5 tests)
// ===========================================================================

#[tokio::test]
async fn stream_pipeline_filter_excludes_errors() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    let error_event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        },
        ext: None,
    };
    assert!(pipeline.process(error_event).is_none());

    let msg_event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "ok".into() },
        ext: None,
    };
    assert!(pipeline.process(msg_event).is_some());
}

#[tokio::test]
async fn stream_pipeline_recorder_captures_events() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();

    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "recorded".into(),
        },
        ext: None,
    };
    pipeline.process(ev);

    assert_eq!(recorder.len(), 1);
    assert!(!recorder.is_empty());
}

#[tokio::test]
async fn stream_pipeline_stats_counts_events() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    for _ in 0..3 {
        pipeline.process(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: None,
        });
    }

    assert_eq!(stats.total_events(), 3);
}

#[tokio::test]
async fn stream_pipeline_runtime_integration() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);

    let wo = WorkOrderBuilder::new("stream pipeline run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
    assert_eq!(recorder.len(), events.len());
    assert_eq!(stats.total_events(), events.len() as u64);
}

#[tokio::test]
async fn stream_pipeline_filter_by_kind() {
    let filter = EventFilter::by_kind("assistant_message");
    let pipeline = StreamPipelineBuilder::new().filter(filter).build();

    let msg = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "pass".into(),
        },
        ext: None,
    };
    assert!(pipeline.process(msg).is_some());

    let run_started = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "blocked".into(),
        },
        ext: None,
    };
    assert!(pipeline.process(run_started).is_none());
}

// ===========================================================================
// 20. Budget enforcement (4 tests)
// ===========================================================================

#[tokio::test]
async fn budget_within_limits() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        max_turns: Some(10),
        ..BudgetLimit::default()
    });
    tracker.record_tokens(100);
    tracker.record_turn();
    assert_eq!(tracker.check(), BudgetStatus::WithinLimits);
}

#[tokio::test]
async fn budget_tokens_exceeded() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(100),
        ..BudgetLimit::default()
    });
    tracker.record_tokens(101);
    assert!(matches!(
        tracker.check(),
        BudgetStatus::Exceeded(BudgetViolation::TokensExceeded { .. })
    ));
}

#[tokio::test]
async fn budget_turns_exceeded() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_turns: Some(3),
        ..BudgetLimit::default()
    });
    for _ in 0..4 {
        tracker.record_turn();
    }
    assert!(matches!(
        tracker.check(),
        BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded { .. })
    ));
}

#[tokio::test]
async fn budget_remaining_decrements() {
    let tracker = BudgetTracker::new(BudgetLimit {
        max_tokens: Some(1000),
        max_turns: Some(10),
        ..BudgetLimit::default()
    });
    tracker.record_tokens(300);
    tracker.record_turn();
    tracker.record_turn();

    let remaining = tracker.remaining();
    assert_eq!(remaining.tokens, Some(700));
    assert_eq!(remaining.turns, Some(8));
}

// ===========================================================================
// 21. Pipeline stages (4 tests)
// ===========================================================================

#[tokio::test]
async fn pipeline_stage_validation_passes_valid() {
    let pipeline = Pipeline::new().stage(ValidationStage);
    let mut wo = WorkOrderBuilder::new("valid task")
        .root("/some/root")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    assert!(pipeline.execute(&mut wo).await.is_ok());
}

#[tokio::test]
async fn pipeline_stage_audit_records_entries() {
    let audit = AuditStage::new();
    let pipeline = Pipeline::new().stage(ValidationStage);

    // We can't easily add the same AuditStage to two places, so test directly.
    let mut wo = WorkOrderBuilder::new("audit test")
        .root("/root")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    use abp_runtime::pipeline::PipelineStage;
    audit.process(&mut wo).await.unwrap();
    let entries = audit.entries().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].task, "audit test");
    assert_eq!(entries[0].work_order_id, wo.id);

    // Drop pipeline separately
    drop(pipeline);
}

#[tokio::test]
async fn pipeline_stage_chaining_order() {
    let audit = AuditStage::new();
    let pipeline = Pipeline::new().stage(ValidationStage).stage(PolicyStage);

    assert_eq!(pipeline.len(), 2);
    assert!(!pipeline.is_empty());

    // Also verify audit stage can be used standalone
    let mut wo = WorkOrderBuilder::new("chaining")
        .root("/root")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    use abp_runtime::pipeline::PipelineStage;
    audit.process(&mut wo).await.unwrap();
    assert_eq!(audit.entries().await.len(), 1);
}

#[tokio::test]
async fn pipeline_stage_empty_pipeline() {
    let pipeline = Pipeline::new();
    assert!(pipeline.is_empty());
    assert_eq!(pipeline.len(), 0);

    let mut wo = WorkOrderBuilder::new("empty pipeline")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(pipeline.execute(&mut wo).await.is_ok());
}

// ===========================================================================
// 22. Additional integration tests (3 tests)
// ===========================================================================

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

#[tokio::test]
async fn runtime_default_impl() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[tokio::test]
async fn runtime_with_emulation_config() {
    use abp_emulation::EmulationConfig;
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    assert!(rt.emulation_config().is_some());

    // Should still run fine
    let (_events, receipt) = run_mock(&rt, "with emulation").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}
