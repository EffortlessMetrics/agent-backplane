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
//! End-to-end integration tests exercising the full pipeline from
//! WorkOrder submission through event streaming to Receipt generation.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

async fn run_mock_simple(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = simple_work_order(task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

// ---------------------------------------------------------------------------
// Custom test backends
// ---------------------------------------------------------------------------

/// A backend that always fails.
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
    name: String,
}

impl ConfigurableBackend {
    fn new(name: &str, count: usize) -> Self {
        Self {
            message_count: count,
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl Backend for ConfigurableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
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
                message: format!("{} starting", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(ev.clone()).await;
        trace.push(ev);

        for i in 0..self.message_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("message {i} from {}", self.name),
                },
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

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

/// A backend that emits various event types in a specific order.
#[derive(Debug, Clone)]
struct RichEventsBackend;

#[async_trait]
impl Backend for RichEventsBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "rich".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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

        let kinds = [
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "thinking".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added main".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("Compiling...".into()),
            },
            AgentEventKind::AssistantDelta {
                text: "partial ".into(),
            },
            AgentEventKind::Warning {
                message: "unused var".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ];

        for kind in kinds {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

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
            usage_raw: serde_json::json!({"note": "rich"}),
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

/// A backend that emits only partial results.
#[derive(Debug, Clone)]
struct PartialBackend;

#[async_trait]
impl Backend for PartialBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial".into(),
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
            kind: AgentEventKind::RunCompleted {
                message: "partial done".into(),
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
            outcome: Outcome::Partial,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend that emits error events but still completes.
#[derive(Debug, Clone)]
struct ErrorEventBackend;

#[async_trait]
impl Backend for ErrorEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error_event".into(),
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
        let mut trace = Vec::new();

        for kind in [
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::Error {
                message: "something went wrong".into(),
                error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
            },
            AgentEventKind::RunCompleted {
                message: "done with errors".into(),
            },
        ] {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

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
            outcome: Outcome::Failed,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend that emits tool calls with errors.
#[derive(Debug, Clone)]
struct ToolErrorBackend;

#[async_trait]
impl Backend for ToolErrorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool_error".into(),
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
        let mut trace = Vec::new();

        for kind in [
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Bash".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "rm -rf /"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Bash".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!("permission denied"),
                is_error: true,
            },
            AgentEventKind::RunCompleted {
                message: "completed after tool error".into(),
            },
        ] {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

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

fn build_runtime_with_all() -> Runtime {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("failing", FailingBackend);
    rt.register_backend(
        "configurable3",
        ConfigurableBackend::new("configurable3", 3),
    );
    rt.register_backend(
        "configurable0",
        ConfigurableBackend::new("configurable0", 0),
    );
    rt.register_backend("rich", RichEventsBackend);
    rt.register_backend("partial", PartialBackend);
    rt.register_backend("error_event", ErrorEventBackend);
    rt.register_backend("tool_error", ToolErrorBackend);
    rt
}

// ===========================================================================
// 1. Happy path tests (15)
// ===========================================================================

#[tokio::test]
async fn happy_submit_work_order_returns_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "hello world").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn happy_receipt_has_valid_hash() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "check hash").await;
    assert!(receipt.receipt_sha256.is_some());
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref().unwrap(), recomputed);
}

#[tokio::test]
async fn happy_receipt_contract_version_matches() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "version check").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn happy_events_are_streamed_before_receipt() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock_simple(&rt, "event check").await;
    assert!(!events.is_empty(), "should receive streamed events");
}

#[tokio::test]
async fn happy_run_started_is_first_event() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock_simple(&rt, "ordering").await;
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event must be RunStarted"
    );
}

#[tokio::test]
async fn happy_run_completed_is_last_event() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock_simple(&rt, "ordering last").await;
    let last = events.last().unwrap();
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last event must be RunCompleted"
    );
}

#[tokio::test]
async fn happy_receipt_trace_matches_events() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock_simple(&rt, "trace match").await;
    assert_eq!(
        events.len(),
        receipt.trace.len(),
        "streamed events and receipt trace should have same length"
    );
}

#[tokio::test]
async fn happy_receipt_work_order_id_is_set() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("wo id test");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn happy_receipt_backend_id_is_mock() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "backend id").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn happy_receipt_timestamps_are_ordered() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "timestamps").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn happy_event_timestamps_are_monotonic() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock_simple(&rt, "monotonic ts").await;
    for pair in events.windows(2) {
        assert!(
            pair[0].ts <= pair[1].ts,
            "event timestamps must be monotonic"
        );
    }
}

#[tokio::test]
async fn happy_mock_emits_assistant_messages() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_mock_simple(&rt, "assistant msgs").await;
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert!(
        msg_count >= 1,
        "mock should emit at least one assistant message"
    );
}

#[tokio::test]
async fn happy_receipt_has_duration() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "duration").await;
    // Duration should be a non-negative value.
    assert!(
        receipt.meta.duration_ms < 60_000,
        "duration should be reasonable"
    );
}

#[tokio::test]
async fn happy_receipt_mode_is_mapped_by_default() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "mode check").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn happy_multiple_sequential_runs_succeed() {
    let rt = Runtime::with_default_backends();
    for i in 0..3 {
        let (_events, receipt) = run_mock_simple(&rt, &format!("run {i}")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ===========================================================================
// 2. Multi-backend tests (10)
// ===========================================================================

#[tokio::test]
async fn multi_mock_and_configurable_both_complete() {
    let rt = build_runtime_with_all();
    let wo1 = simple_work_order("multi test 1");
    let wo2 = simple_work_order("multi test 2");
    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let h2 = rt.run_streaming("configurable3", wo2).await.unwrap();
    let (_, r1) = drain_run(h1).await;
    let (_, r2) = drain_run(h2).await;
    assert_eq!(r1.unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn multi_receipts_have_different_backend_ids() {
    let rt = build_runtime_with_all();
    let h1 = rt
        .run_streaming("mock", simple_work_order("multi id 1"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("configurable3", simple_work_order("multi id 2"))
        .await
        .unwrap();
    let (_, r1) = drain_run(h1).await;
    let (_, r2) = drain_run(h2).await;
    assert_ne!(r1.unwrap().backend.id, r2.unwrap().backend.id);
}

#[tokio::test]
async fn multi_all_backends_produce_valid_hashes() {
    let rt = build_runtime_with_all();
    for name in ["mock", "configurable3", "rich", "partial"] {
        let handle = rt
            .run_streaming(name, simple_work_order(&format!("{name} hash")))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert!(
            receipt.receipt_sha256.is_some(),
            "backend {name} missing hash"
        );
        let recomputed = receipt_hash(&receipt).unwrap();
        assert_eq!(
            receipt.receipt_sha256.as_deref().unwrap(),
            recomputed,
            "hash mismatch for backend {name}"
        );
    }
}

#[tokio::test]
async fn multi_all_backends_include_contract_version() {
    let rt = build_runtime_with_all();
    for name in ["mock", "configurable3", "rich", "partial"] {
        let handle = rt
            .run_streaming(name, simple_work_order(&format!("{name} version")))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(
            receipt.unwrap().meta.contract_version,
            CONTRACT_VERSION,
            "contract version mismatch for backend {name}"
        );
    }
}

#[tokio::test]
async fn multi_configurable_emits_expected_message_count() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("configurable3", simple_work_order("count msgs"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 3);
}

#[tokio::test]
async fn multi_zero_message_backend_still_produces_receipt() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("configurable0", simple_work_order("zero msg"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    // Should still have RunStarted and RunCompleted.
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 0);
}

#[tokio::test]
async fn multi_partial_backend_returns_partial_outcome() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("partial", simple_work_order("partial"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Partial);
}

#[tokio::test]
async fn multi_rich_backend_emits_diverse_events() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("rich events"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let has_tool_call = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    let has_tool_result = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }));
    let has_file_changed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. }));
    assert!(has_tool_call, "rich backend should emit ToolCall");
    assert!(has_tool_result, "rich backend should emit ToolResult");
    assert!(has_file_changed, "rich backend should emit FileChanged");
}

#[tokio::test]
async fn multi_different_backends_produce_different_hashes() {
    let rt = build_runtime_with_all();
    let h1 = rt
        .run_streaming("mock", simple_work_order("hash diff"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("rich", simple_work_order("hash diff"))
        .await
        .unwrap();
    let (_, r1) = drain_run(h1).await;
    let (_, r2) = drain_run(h2).await;
    assert_ne!(
        r1.unwrap().receipt_sha256,
        r2.unwrap().receipt_sha256,
        "different backends should produce different hashes"
    );
}

#[tokio::test]
async fn multi_same_backend_different_tasks_produce_different_receipts() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("task alpha"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("task beta"))
        .await
        .unwrap();
    let (_, r1) = drain_run(h1).await;
    let (_, r2) = drain_run(h2).await;
    // Work order IDs differ so receipts differ.
    assert_ne!(
        r1.unwrap().meta.work_order_id,
        r2.unwrap().meta.work_order_id
    );
}

// ===========================================================================
// 3. Error recovery tests (15)
// ===========================================================================

#[tokio::test]
async fn error_unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let result = rt
        .run_streaming("nonexistent", simple_work_order("fail"))
        .await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );
}

#[tokio::test]
async fn error_unknown_backend_name_preserved() {
    let rt = Runtime::new();
    let err = rt
        .run_streaming("xyz", simple_work_order("fail"))
        .await
        .err()
        .unwrap();
    match err {
        RuntimeError::UnknownBackend { name } => assert_eq!(name, "xyz"),
        other => panic!("expected UnknownBackend, got {other:?}"),
    }
}

#[tokio::test]
async fn error_failing_backend_returns_backend_failed() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("failing", simple_work_order("should fail"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn error_failing_backend_error_is_backend_failed() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("failing", simple_work_order("backend failed"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    match receipt {
        Err(RuntimeError::BackendFailed(_)) => {} // expected
        other => panic!("expected BackendFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn error_unsatisfied_capability_native() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("cap test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn error_unsatisfied_capability_error_code() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("cap code test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpServer,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let err = rt.run_streaming("mock", wo).await.err().unwrap();
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[tokio::test]
async fn error_empty_backend_name_is_unknown() {
    let rt = Runtime::with_default_backends();
    let result = rt.run_streaming("", simple_work_order("empty")).await;
    assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
}

#[tokio::test]
async fn error_multiple_unknown_backend_attempts() {
    let rt = Runtime::with_default_backends();
    for name in ["a", "b", "c"] {
        let result = rt.run_streaming(name, simple_work_order("fail")).await;
        assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
    }
}

#[tokio::test]
async fn error_runtime_error_has_error_code() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[tokio::test]
async fn error_backend_failed_has_error_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[tokio::test]
async fn error_workspace_failed_has_error_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("no disk"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[tokio::test]
async fn error_policy_failed_has_error_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[tokio::test]
async fn error_error_event_backend_still_returns_receipt() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("error_event", simple_work_order("error event"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Failed);
    let has_error = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(has_error, "should have Error events");
}

#[tokio::test]
async fn error_tool_error_backend_still_completes() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("tool_error", simple_work_order("tool error"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn error_into_abp_error_preserves_code() {
    let err = RuntimeError::UnknownBackend {
        name: "gone".into(),
    };
    let code = err.error_code();
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, code);
}

// ===========================================================================
// 4. Event stream validation tests (10)
// ===========================================================================

#[tokio::test]
async fn stream_run_started_always_first_mock() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock_simple(&rt, "first event").await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn stream_run_completed_always_last_mock() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock_simple(&rt, "last event").await;
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn stream_run_started_always_first_rich() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("rich first"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn stream_run_completed_always_last_rich() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("rich last"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn stream_rich_contains_tool_call_before_tool_result() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("tool order"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let tc_pos = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .expect("should have ToolCall");
    let tr_pos = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }))
        .expect("should have ToolResult");
    assert!(tc_pos < tr_pos, "ToolCall must precede ToolResult");
}

#[tokio::test]
async fn stream_rich_event_type_distribution() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("distribution"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;

    let mut started = 0;
    let mut completed = 0;
    let mut messages = 0;
    let mut deltas = 0;
    let mut tool_calls = 0;
    let mut tool_results = 0;
    let mut file_changed = 0;
    let mut commands = 0;
    let mut warnings = 0;

    for ev in &events {
        match &ev.kind {
            AgentEventKind::RunStarted { .. } => started += 1,
            AgentEventKind::RunCompleted { .. } => completed += 1,
            AgentEventKind::AssistantMessage { .. } => messages += 1,
            AgentEventKind::AssistantDelta { .. } => deltas += 1,
            AgentEventKind::ToolCall { .. } => tool_calls += 1,
            AgentEventKind::ToolResult { .. } => tool_results += 1,
            AgentEventKind::FileChanged { .. } => file_changed += 1,
            AgentEventKind::CommandExecuted { .. } => commands += 1,
            AgentEventKind::Warning { .. } => warnings += 1,
            _ => {}
        }
    }

    assert_eq!(started, 1, "exactly one RunStarted");
    assert_eq!(completed, 1, "exactly one RunCompleted");
    assert_eq!(messages, 1, "one AssistantMessage");
    assert_eq!(deltas, 1, "one AssistantDelta");
    assert_eq!(tool_calls, 1, "one ToolCall");
    assert_eq!(tool_results, 1, "one ToolResult");
    assert_eq!(file_changed, 1, "one FileChanged");
    assert_eq!(commands, 1, "one CommandExecuted");
    assert_eq!(warnings, 1, "one Warning");
}

#[tokio::test]
async fn stream_mock_event_count() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock_simple(&rt, "mock events").await;
    // MockBackend emits: RunStarted, 2× AssistantMessage, RunCompleted = 4
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn stream_no_duplicate_run_started() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("dup start"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .count();
    assert_eq!(count, 1, "should have exactly one RunStarted");
}

#[tokio::test]
async fn stream_no_duplicate_run_completed() {
    let rt = build_runtime_with_all();
    let handle = rt
        .run_streaming("rich", simple_work_order("dup complete"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .count();
    assert_eq!(count, 1, "should have exactly one RunCompleted");
}

#[tokio::test]
async fn stream_all_events_have_timestamps() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock_simple(&rt, "ts check").await;
    for ev in &events {
        // All events have a `ts` field; verify it's a sensible value.
        assert!(ev.ts.timestamp() > 0, "event timestamp should be positive");
    }
}

// ===========================================================================
// 5. Receipt validation tests (10)
// ===========================================================================

#[tokio::test]
async fn receipt_hash_is_deterministic_for_same_receipt() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "deterministic hash").await;
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2, "receipt hash must be deterministic");
}

#[tokio::test]
async fn receipt_hash_is_64_hex_chars() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "hash length").await;
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest is 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should be hex"
    );
}

#[tokio::test]
async fn receipt_hash_excludes_self_from_computation() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "self-ref hash").await;
    // The hash computation sets receipt_sha256 to null before hashing.
    // Verify by recomputing on the filled receipt.
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref().unwrap(), recomputed);
}

#[tokio::test]
async fn receipt_contract_version_is_abp_v01() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "contract version").await;
    assert_eq!(receipt.meta.contract_version, "abp/v0.1");
}

#[tokio::test]
async fn receipt_work_order_id_correlation() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("correlation");
    let expected_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().meta.work_order_id, expected_id);
}

#[tokio::test]
async fn receipt_duration_is_non_negative() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "duration check").await;
    // duration_ms is u64, so always >= 0, but check reasonability.
    assert!(receipt.meta.duration_ms < 30_000);
}

#[tokio::test]
async fn receipt_finished_at_after_started_at() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "timing order").await;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn receipt_serializes_to_json() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "json serde").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.work_order_id, receipt.meta.work_order_id);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
}

#[tokio::test]
async fn receipt_outcome_serializes_snake_case() {
    let json = serde_json::to_value(Outcome::Complete).unwrap();
    assert_eq!(json, serde_json::json!("complete"));
    let json = serde_json::to_value(Outcome::Partial).unwrap();
    assert_eq!(json, serde_json::json!("partial"));
    let json = serde_json::to_value(Outcome::Failed).unwrap();
    assert_eq!(json, serde_json::json!("failed"));
}

#[tokio::test]
async fn receipt_run_id_is_non_nil() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "run id check").await;
    assert!(!receipt.meta.run_id.is_nil(), "run_id should not be nil");
}

// ===========================================================================
// Additional coverage tests (to reach 60+)
// ===========================================================================

#[tokio::test]
async fn backend_names_includes_mock() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn backend_names_reflects_registrations() {
    let rt = build_runtime_with_all();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
    assert!(names.contains(&"failing".to_string()));
    assert!(names.contains(&"rich".to_string()));
}

#[tokio::test]
async fn empty_runtime_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[tokio::test]
async fn register_backend_replaces_existing() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("mock", ConfigurableBackend::new("mock", 5));
    let handle = rt
        .run_streaming("mock", simple_work_order("replaced"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // The configurable backend emits 5 messages + RunStarted + RunCompleted.
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert_eq!(msg_count, 5);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("build task").build();
    assert_eq!(wo.task, "build task");
}

#[tokio::test]
async fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("model task").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[tokio::test]
async fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("turns task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[tokio::test]
async fn work_order_builder_sets_max_budget() {
    let wo = WorkOrderBuilder::new("budget task")
        .max_budget_usd(1.0)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
}

#[tokio::test]
async fn work_order_has_unique_ids() {
    let wo1 = simple_work_order("id1");
    let wo2 = simple_work_order("id2");
    assert_ne!(wo1.id, wo2.id, "work order IDs should be unique");
}

#[tokio::test]
async fn receipt_builder_produces_valid_receipt() {
    let receipt = abp_core::ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.backend.id, "test-backend");
}

#[tokio::test]
async fn receipt_chain_accumulates_receipts() {
    let rt = Runtime::with_default_backends();
    let _r1 = run_mock_simple(&rt, "chain 1").await;
    let _r2 = run_mock_simple(&rt, "chain 2").await;
    let chain = rt.receipt_chain();
    let chain = chain.lock().await;
    assert!(chain.len() >= 2, "receipt chain should accumulate");
}

#[tokio::test]
async fn error_code_serializes_snake_case() {
    let code = abp_error::ErrorCode::BackendNotFound;
    let json = serde_json::to_value(code).unwrap();
    assert_eq!(json, serde_json::json!("backend_not_found"));
}

#[tokio::test]
async fn error_code_display_uses_message() {
    let code = abp_error::ErrorCode::BackendTimeout;
    assert_eq!(code.to_string(), code.message());
}

#[tokio::test]
async fn error_code_as_str_is_snake_case() {
    let code = abp_error::ErrorCode::BackendCrashed;
    assert_eq!(code.as_str(), "backend_crashed");
}

#[tokio::test]
async fn error_code_retryable_for_timeout() {
    assert!(abp_error::ErrorCode::BackendTimeout.is_retryable());
    assert!(abp_error::ErrorCode::BackendRateLimited.is_retryable());
    assert!(!abp_error::ErrorCode::BackendNotFound.is_retryable());
}

#[tokio::test]
async fn mock_backend_capabilities_include_streaming() {
    let mock = abp_integrations::MockBackend;
    let caps = mock.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn mock_backend_identity() {
    let mock = abp_integrations::MockBackend;
    assert_eq!(mock.identity().id, "mock");
}

#[tokio::test]
async fn receipt_verification_report_defaults() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "verification").await;
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn receipt_artifacts_empty_for_mock() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "artifacts").await;
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
async fn receipt_usage_fields_for_mock() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_mock_simple(&rt, "usage").await;
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn concurrent_runs_on_same_runtime() {
    let rt = std::sync::Arc::new(build_runtime_with_all());
    let mut handles = Vec::new();
    for i in 0..5 {
        let rt = rt.clone();
        handles.push(tokio::spawn(async move {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("concurrent {i}")))
                .await
                .unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap()
        }));
    }
    for h in handles {
        let receipt = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn run_handle_run_id_is_non_nil() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", simple_work_order("run handle"))
        .await
        .unwrap();
    assert!(!handle.run_id.is_nil());
    let _ = drain_run(handle).await;
}
