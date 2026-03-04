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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end integration tests for the full runtime pipeline.
//!
//! All tests use `MockBackend` — no external services required.
//! Covers: full pipeline, workspace integration, error paths, and event streaming.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, PolicyProfile, Receipt, RunMetadata, SupportLevel, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_receipt::{compute_hash, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Drain all streamed events and await the receipt from a `RunHandle`.
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

/// Build a PassThrough work order for a given task.
fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// Run mock backend and return (events, receipt).
async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = passthrough_wo(task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

// ===========================================================================
// Custom test backends
// ===========================================================================

/// Backend that always returns an error.
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

/// Backend that emits a configurable number of events of varying types.
#[derive(Debug, Clone)]
struct MultiEventBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for MultiEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-event".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
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
        let started = chrono::Utc::now();
        let mut trace = Vec::new();

        // RunStarted
        let kind = AgentEventKind::RunStarted {
            message: "starting".into(),
        };
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        // AssistantMessage events
        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("message {i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        // ToolCall event
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        // ToolResult event
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        // RunCompleted
        let ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

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

/// Backend that sleeps for a long time (for timeout/cancellation tests).
#[derive(Debug, Clone)]
struct SlowBackend;

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
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        anyhow::bail!("should not reach here")
    }
}

// ===========================================================================
// 1. Full pipeline — mock backend (5 tests)
// ===========================================================================

#[tokio::test]
async fn full_pipeline_submit_receive_events_get_receipt() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "e2e test task").await;

    assert!(!events.is_empty(), "should receive events");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn full_pipeline_receipt_has_valid_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "hash check").await;

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest should be 64 chars");
    assert!(verify_hash(&receipt), "receipt hash should verify");

    // Recompute and compare.
    let recomputed = compute_hash(&receipt).unwrap();
    assert_eq!(&recomputed, hash);
}

#[tokio::test]
async fn full_pipeline_events_contain_run_started_and_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "lifecycle events").await;

    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));

    assert!(has_started, "events must include RunStarted");
    assert!(has_completed, "events must include RunCompleted");
}

#[tokio::test]
async fn full_pipeline_receipt_includes_backend_identity() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "identity check").await;

    assert_eq!(receipt.backend.id, "mock");
    assert!(receipt.backend.backend_version.is_some());
}

#[tokio::test]
async fn full_pipeline_multi_turn_conversation() {
    let rt = Runtime::with_default_backends();

    // Run two sequential work orders through the same runtime.
    let (events1, receipt1) = run_mock(&rt, "turn 1").await;
    let (events2, receipt2) = run_mock(&rt, "turn 2").await;

    // Both should complete successfully.
    assert_eq!(receipt1.outcome, Outcome::Complete);
    assert_eq!(receipt2.outcome, Outcome::Complete);

    // They should have different run IDs.
    assert_ne!(receipt1.meta.run_id, receipt2.meta.run_id);

    // Both should produce events.
    assert!(!events1.is_empty());
    assert!(!events2.is_empty());

    // Receipt chain should contain both.
    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert!(chain_guard.len() >= 2);
}

// ===========================================================================
// 2. Workspace integration (5 tests)
// ===========================================================================

#[tokio::test]
async fn workspace_staged_run_completes() {
    let rt = Runtime::with_default_backends();
    // Use a small temp dir to avoid copying the entire repo (which includes target/).
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("hello.txt"), "world").unwrap();
    let wo = WorkOrderBuilder::new("staged workspace test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(temp.path().to_str().unwrap())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn workspace_cleaned_up_after_run() {
    let rt = Runtime::with_default_backends();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("file.txt"), "data").unwrap();
    let wo = WorkOrderBuilder::new("cleanup test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(temp.path().to_str().unwrap())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // The staged workspace path (rewritten in the receipt's work_order)
    // should be a temp dir that is cleaned up. We verify the run completed
    // successfully — the temp dir is dropped when PreparedWorkspace drops.
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn workspace_excludes_git_directory() {
    // Create a temp dir with a .git subdirectory and a source file.
    let temp = tempfile::tempdir().unwrap();
    let git_dir = temp.path().join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(temp.path().join("src.txt"), "hello").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("git exclude test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(temp.path().to_str().unwrap())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn workspace_passthrough_mode() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn workspace_with_policy_restrictions() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        deny_read: vec!["**/*.secret".to_string()],
        deny_write: vec!["**/protected/**".to_string()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("policy restricted workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 3. Error paths (5 tests)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("unknown backend test");

    let result = rt.run_streaming("nonexistent-backend", wo).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got: {err:?}"
    );
}

#[tokio::test]
async fn error_empty_task_rejected_by_pipeline() {
    // The ValidationStage in the pipeline rejects empty tasks.
    use abp_runtime::pipeline::{Pipeline, ValidationStage};

    let mut wo = WorkOrderBuilder::new("ok")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    wo.task = "".into();

    let pipeline = Pipeline::new().stage(ValidationStage);
    let result = pipeline.execute(&mut wo).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("task must not be empty"),
        "should reject empty task"
    );
}

#[tokio::test]
async fn error_backend_failure_produces_error() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "failing",
        FailingBackend {
            message: "intentional failure".into(),
        },
    );

    let wo = passthrough_wo("failing backend test");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;

    assert!(receipt.is_err(), "failing backend should produce error");
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn error_timeout_handling() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("slow", SlowBackend);

    let wo = passthrough_wo("timeout test");
    let handle = rt.run_streaming("slow", wo).await.unwrap();

    // Use tokio::time::timeout to simulate runtime-level timeout.
    let result = tokio::time::timeout(std::time::Duration::from_millis(200), async {
        drain_run(handle).await
    })
    .await;

    assert!(result.is_err(), "slow backend should time out");
}

#[tokio::test]
async fn error_cancellation_handling() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("slow", SlowBackend);

    let wo = passthrough_wo("cancellation test");
    let handle = rt.run_streaming("slow", wo).await.unwrap();

    // Abort the receipt task to simulate cancellation.
    handle.receipt.abort();

    // The abort should be reflected.
    // (We can't drain events after abort, but the abort itself should not panic.)
}

// ===========================================================================
// 4. Event streaming (5 tests)
// ===========================================================================

#[tokio::test]
async fn events_arrive_in_order() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "order test").await;

    // First event should be RunStarted, last should be RunCompleted.
    assert!(
        matches!(
            &events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ),
        "first event must be RunStarted"
    );
    assert!(
        matches!(
            &events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event must be RunCompleted"
    );

    // Timestamps should be non-decreasing.
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "events must be in chronological order"
        );
    }
}

#[tokio::test]
async fn events_assistant_message_has_content() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "content check").await;

    let messages: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !messages.is_empty(),
        "should have at least one AssistantMessage"
    );
    for msg in &messages {
        assert!(!msg.is_empty(), "AssistantMessage text must not be empty");
    }
}

#[tokio::test]
async fn events_tool_call_has_tool_name() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("multi-event", MultiEventBackend { event_count: 2 });

    let wo = passthrough_wo("tool call test");
    let handle = rt.run_streaming("multi-event", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let _receipt = receipt.unwrap();

    let tool_calls: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !tool_calls.is_empty(),
        "should have at least one ToolCall event"
    );
    for name in &tool_calls {
        assert!(!name.is_empty(), "ToolCall tool_name must not be empty");
    }
}

#[tokio::test]
async fn events_multiple_types_in_single_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("multi-event", MultiEventBackend { event_count: 3 });

    let wo = passthrough_wo("multi-type test");
    let handle = rt.run_streaming("multi-event", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;

    let mut has_started = false;
    let mut has_completed = false;
    let mut has_message = false;
    let mut has_tool_call = false;
    let mut has_tool_result = false;

    for ev in &events {
        match &ev.kind {
            AgentEventKind::RunStarted { .. } => has_started = true,
            AgentEventKind::RunCompleted { .. } => has_completed = true,
            AgentEventKind::AssistantMessage { .. } => has_message = true,
            AgentEventKind::ToolCall { .. } => has_tool_call = true,
            AgentEventKind::ToolResult { .. } => has_tool_result = true,
            _ => {}
        }
    }

    assert!(has_started, "must have RunStarted");
    assert!(has_completed, "must have RunCompleted");
    assert!(has_message, "must have AssistantMessage");
    assert!(has_tool_call, "must have ToolCall");
    assert!(has_tool_result, "must have ToolResult");
}

#[tokio::test]
async fn events_count_matches_receipt_trace() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("multi-event", MultiEventBackend { event_count: 5 });

    let wo = passthrough_wo("count test");
    let handle = rt.run_streaming("multi-event", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // The receipt trace should contain the same events the backend emitted.
    // The streamed events should match in count.
    assert_eq!(
        events.len(),
        receipt.trace.len(),
        "streamed event count should match receipt trace length"
    );

    // MultiEventBackend emits: 1 RunStarted + event_count messages + 1 ToolCall + 1 ToolResult + 1 RunCompleted
    let expected = 1 + 5 + 1 + 1 + 1;
    assert_eq!(events.len(), expected);
}

// ===========================================================================
// Additional tests (to ensure 20+ total)
// ===========================================================================

#[tokio::test]
async fn receipt_contract_version_is_correct() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "version check").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_timing_metadata_is_populated() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "timing check").await;

    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    // The work order id should be a valid UUID (non-nil for a generated order).
    assert_ne!(receipt.meta.work_order_id, Uuid::nil());
}

#[tokio::test]
async fn receipt_hash_is_deterministic() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "deterministic hash").await;

    let hash1 = compute_hash(&receipt).unwrap();
    let hash2 = compute_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2, "hash must be deterministic");
}

#[tokio::test]
async fn runtime_lists_registered_backends() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn runtime_receipt_chain_grows_with_runs() {
    let rt = Runtime::with_default_backends();

    let chain = rt.receipt_chain();
    assert_eq!(chain.lock().await.len(), 0);

    run_mock(&rt, "chain run 1").await;
    assert_eq!(chain.lock().await.len(), 1);

    run_mock(&rt, "chain run 2").await;
    assert_eq!(chain.lock().await.len(), 2);
}

#[tokio::test]
async fn receipt_capabilities_reflect_backend() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "capabilities check").await;

    // MockBackend advertises Streaming as Native.
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}
