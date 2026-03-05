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
//! Comprehensive streaming event pipeline tests.
//!
//! 60+ tests verifying event ordering, event types/content, stream lifecycle,
//! and runtime integration for the ABP streaming pipeline.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::Runtime;
use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipelineBuilder,
};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

async fn emit(trace: &mut Vec<AgentEvent>, tx: &mpsc::Sender<AgentEvent>, kind: AgentEventKind) {
    let ev = make_event(kind);
    trace.push(ev.clone());
    let _ = tx.send(ev).await;
}

fn build_receipt(
    run_id: Uuid,
    work_order: &WorkOrder,
    trace: Vec<AgentEvent>,
    started: chrono::DateTime<Utc>,
    outcome: Outcome,
) -> anyhow::Result<Receipt> {
    let finished = Utc::now();
    let receipt = Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: work_order.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
        },
        backend: BackendIdentity {
            id: "pipeline-test".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    };
    receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
}

/// Drain all streamed events and await the receipt from a RunHandle.
async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, abp_runtime::RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("backend task panicked");
    (collected, receipt)
}

fn kind_name(ev: &AgentEvent) -> &'static str {
    match &ev.kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::FileChanged { .. } => "file_changed",
        AgentEventKind::CommandExecuted { .. } => "command_executed",
        AgentEventKind::Warning { .. } => "warning",
        AgentEventKind::Error { .. } => "error",
    }
}

// ===========================================================================
// Custom Backends
// ===========================================================================

/// Emits numbered AssistantDelta events for ordering verification.
#[derive(Debug, Clone)]
struct OrderedDeltaBackend {
    count: usize,
}

#[async_trait]
impl Backend for OrderedDeltaBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "ordered-delta".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        for i in 0..self.count {
            emit(
                &mut trace,
                &tx,
                AgentEventKind::AssistantDelta {
                    text: format!("delta-{i}"),
                },
            )
            .await;
        }
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Emits a full mixed sequence of event types.
#[derive(Debug, Clone)]
struct FullMixedBackend;

#[async_trait]
impl Backend for FullMixedBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "full-mixed".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "hello ".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "main.rs"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "refactored".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("all tests passed".into()),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "unused variable".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "world".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Backend that emits only an error event and then completes with Failed.
#[derive(Debug, Clone)]
struct ErrorOnlyBackend;

#[async_trait]
impl Backend for ErrorOnlyBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-only".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Error {
                message: "something went wrong".into(),
                error_code: None,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "failed".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Failed)
    }
}

/// Backend that emits nothing but RunStarted/RunCompleted.
#[derive(Debug, Clone)]
struct MinimalBackend;

#[async_trait]
impl Backend for MinimalBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "minimal".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Backend that emits many tool calls with IDs.
#[derive(Debug, Clone)]
struct MultiToolBackend {
    tool_count: usize,
}

#[async_trait]
impl Backend for MultiToolBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-tool".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        for i in 0..self.tool_count {
            let tool_id = format!("tc-{i}");
            emit(
                &mut trace,
                &tx,
                AgentEventKind::ToolCall {
                    tool_name: format!("tool_{i}"),
                    tool_use_id: Some(tool_id.clone()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"arg": i}),
                },
            )
            .await;
            emit(
                &mut trace,
                &tx,
                AgentEventKind::ToolResult {
                    tool_name: format!("tool_{i}"),
                    tool_use_id: Some(tool_id),
                    output: serde_json::json!({"result": i * 2}),
                    is_error: false,
                },
            )
            .await;
        }
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Backend that returns an error (not a receipt).
#[derive(Debug, Clone)]
struct FailingBackend;

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
            backend_version: Some("0.1".into()),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }))
            .await;
        let _ = tx
            .send(make_event(AgentEventKind::Error {
                message: "backend crashed".into(),
                error_code: None,
            }))
            .await;
        anyhow::bail!("simulated crash")
    }
}

/// Backend that emits duplicate events.
#[derive(Debug, Clone)]
struct DuplicateEventBackend;

#[async_trait]
impl Backend for DuplicateEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "duplicate".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        // Send the same delta twice
        for _ in 0..2 {
            emit(
                &mut trace,
                &tx,
                AgentEventKind::AssistantDelta { text: "dup".into() },
            )
            .await;
        }
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Backend with nested tool calls (parent_tool_use_id set).
#[derive(Debug, Clone)]
struct NestedToolBackend;

#[async_trait]
impl Backend for NestedToolBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "nested-tool".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "outer".into(),
                tool_use_id: Some("tc-outer".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "inner".into(),
                tool_use_id: Some("tc-inner".into()),
                parent_tool_use_id: Some("tc-outer".into()),
                input: serde_json::json!({"nested": true}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "inner".into(),
                tool_use_id: Some("tc-inner".into()),
                output: serde_json::json!({"ok": true}),
                is_error: false,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "outer".into(),
                tool_use_id: Some("tc-outer".into()),
                output: serde_json::json!({"ok": true}),
                is_error: false,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Complete)
    }
}

/// Backend with multiple warnings and errors mixed in.
#[derive(Debug, Clone)]
struct WarningErrorBackend;

#[async_trait]
impl Backend for WarningErrorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "warn-err".into(),
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
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "warn-1".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: "text".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "warn-2".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Error {
                message: "err-1".into(),
                error_code: None,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        )
        .await;
        build_receipt(run_id, &work_order, trace, started, Outcome::Partial)
    }
}

// ===========================================================================
// 1. EVENT ORDERING (15+ tests)
// ===========================================================================

#[tokio::test]
async fn ordering_events_arrive_in_emission_order() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 10 });
    let handle = rt
        .run_streaming("ordered", make_work_order("ordering"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
    // Check deltas are in order 0..9
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let expected: Vec<String> = (0..10).map(|i| format!("delta-{i}")).collect();
    assert_eq!(deltas, expected);
}

#[tokio::test]
async fn ordering_timestamps_are_non_decreasing() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 5 });
    let handle = rt
        .run_streaming("ordered", make_work_order("ts order"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for pair in events.windows(2) {
        assert!(
            pair[0].ts <= pair[1].ts,
            "timestamps should be non-decreasing"
        );
    }
}

#[tokio::test]
async fn ordering_run_started_is_first_event() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("first event"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(!events.is_empty());
    assert!(
        matches!(events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event must be RunStarted, got {:?}",
        kind_name(&events[0])
    );
}

#[tokio::test]
async fn ordering_run_completed_is_last_event() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("last event"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let last = events.last().unwrap();
    assert!(
        matches!(last.kind, AgentEventKind::RunCompleted { .. }),
        "last event must be RunCompleted, got {:?}",
        kind_name(last)
    );
}

#[tokio::test]
async fn ordering_run_started_is_first_with_mock_backend() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("mock first"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn ordering_run_completed_is_last_with_mock_backend() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("mock last"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn ordering_deltas_between_run_started_and_completed() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 3 });
    let handle = rt
        .run_streaming("ordered", make_work_order("between"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    for ev in &events[1..events.len() - 1] {
        assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    }
}

#[tokio::test]
async fn ordering_many_events_preserve_sequence() {
    let mut rt = Runtime::new();
    rt.register_backend("big", OrderedDeltaBackend { count: 100 });
    let handle = rt
        .run_streaming("big", make_work_order("100 events"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    // 100 deltas + RunStarted + RunCompleted = 102
    assert_eq!(events.len(), 102);
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 100);
    for (i, d) in deltas.iter().enumerate() {
        assert_eq!(d, &format!("delta-{i}"));
    }
}

#[tokio::test]
async fn ordering_duplicate_events_are_delivered() {
    let mut rt = Runtime::new();
    rt.register_backend("dup", DuplicateEventBackend);
    let handle = rt
        .run_streaming("dup", make_work_order("duplicates"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let dup_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { text } if text == "dup"))
        .count();
    assert_eq!(dup_count, 2, "duplicate events should both be delivered");
}

#[tokio::test]
async fn ordering_tool_call_before_result() {
    let mut rt = Runtime::new();
    rt.register_backend("tools", MultiToolBackend { tool_count: 3 });
    let handle = rt
        .run_streaming("tools", make_work_order("tool order"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let tool_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
            )
        })
        .collect();
    // Should be [call, result, call, result, call, result]
    for chunk in tool_events.chunks(2) {
        assert!(matches!(chunk[0].kind, AgentEventKind::ToolCall { .. }));
        assert!(matches!(chunk[1].kind, AgentEventKind::ToolResult { .. }));
    }
}

#[tokio::test]
async fn ordering_nested_tool_calls_maintain_sequence() {
    let mut rt = Runtime::new();
    rt.register_backend("nested", NestedToolBackend);
    let handle = rt
        .run_streaming("nested", make_work_order("nested"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let tool_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
            )
        })
        .collect();
    assert_eq!(tool_events.len(), 4);
    // outer call, inner call, inner result, outer result
    assert!(
        matches!(&tool_events[0].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "outer")
    );
    assert!(
        matches!(&tool_events[1].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "inner")
    );
    assert!(
        matches!(&tool_events[2].kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "inner")
    );
    assert!(
        matches!(&tool_events[3].kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "outer")
    );
}

#[tokio::test]
async fn ordering_error_event_not_necessarily_last_of_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("warn-err", WarningErrorBackend);
    let handle = rt
        .run_streaming("warn-err", make_work_order("error pos"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    // Error is not last — RunCompleted is last
    let err_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .unwrap();
    assert!(err_idx < events.len() - 1);
}

#[tokio::test]
async fn ordering_mixed_event_types_maintain_emission_order() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("mixed order"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let names: Vec<_> = events.iter().map(kind_name).collect();
    assert_eq!(
        names,
        vec![
            "run_started",
            "assistant_delta",
            "tool_call",
            "tool_result",
            "file_changed",
            "command_executed",
            "warning",
            "assistant_delta",
            "assistant_message",
            "run_completed",
        ]
    );
}

#[tokio::test]
async fn ordering_multiplexer_sorts_by_timestamp() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    let base = Utc::now();
    let t1 = base + chrono::Duration::milliseconds(10);
    let t2 = base + chrono::Duration::milliseconds(5);
    let t3 = base + chrono::Duration::milliseconds(15);
    let t4 = base + chrono::Duration::milliseconds(1);

    tx1.send(make_event_at(
        AgentEventKind::AssistantDelta { text: "a".into() },
        t1,
    ))
    .await
    .unwrap();
    tx1.send(make_event_at(
        AgentEventKind::AssistantDelta { text: "c".into() },
        t3,
    ))
    .await
    .unwrap();
    drop(tx1);

    tx2.send(make_event_at(
        AgentEventKind::AssistantDelta { text: "d".into() },
        t4,
    ))
    .await
    .unwrap();
    tx2.send(make_event_at(
        AgentEventKind::AssistantDelta { text: "b".into() },
        t2,
    ))
    .await
    .unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let sorted = mux.collect_sorted().await;
    let texts: Vec<_> = sorted
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["d", "b", "a", "c"]);
}

#[tokio::test]
async fn ordering_multiplexer_merge_channel() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    let base = Utc::now();
    tx1.send(make_event_at(
        AgentEventKind::AssistantDelta {
            text: "second".into(),
        },
        base + chrono::Duration::milliseconds(20),
    ))
    .await
    .unwrap();
    drop(tx1);

    tx2.send(make_event_at(
        AgentEventKind::AssistantDelta {
            text: "first".into(),
        },
        base + chrono::Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged_rx = mux.merge(16);
    let mut results = Vec::new();
    while let Some(ev) = merged_rx.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 2);
    // Merge delivers all events; ordering depends on channel scheduling.
}

// ===========================================================================
// 2. EVENT TYPES AND CONTENT (15+ tests)
// ===========================================================================

#[tokio::test]
async fn content_assistant_delta_produces_incremental_text() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", OrderedDeltaBackend { count: 3 });
    let handle = rt
        .run_streaming("ordered", make_work_order("deltas"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["delta-0", "delta-1", "delta-2"]);
}

#[tokio::test]
async fn content_tool_call_carries_correct_ids() {
    let mut rt = Runtime::new();
    rt.register_backend("tools", MultiToolBackend { tool_count: 2 });
    let handle = rt
        .run_streaming("tools", make_work_order("tool ids"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let calls: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => Some((tool_name.clone(), tool_use_id.clone(), input.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "tool_0");
    assert_eq!(calls[0].1, Some("tc-0".to_string()));
    assert_eq!(calls[0].2, serde_json::json!({"arg": 0}));
    assert_eq!(calls[1].0, "tool_1");
    assert_eq!(calls[1].1, Some("tc-1".to_string()));
}

#[tokio::test]
async fn content_tool_result_references_correct_tool_call_id() {
    let mut rt = Runtime::new();
    rt.register_backend("tools", MultiToolBackend { tool_count: 2 });
    let handle = rt
        .run_streaming("tools", make_work_order("result ids"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let results: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolResult {
                tool_name,
                tool_use_id,
                output,
                is_error,
            } => Some((
                tool_name.clone(),
                tool_use_id.clone(),
                output.clone(),
                *is_error,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "tool_0");
    assert_eq!(results[0].1, Some("tc-0".to_string()));
    assert_eq!(results[0].2, serde_json::json!({"result": 0}));
    assert!(!results[0].3);
    assert_eq!(results[1].1, Some("tc-1".to_string()));
}

#[tokio::test]
async fn content_file_changed_has_valid_path() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("file changed"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let files: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::FileChanged { path, summary } => Some((path.clone(), summary.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, "src/lib.rs");
    assert!(!files[0].1.is_empty());
}

#[tokio::test]
async fn content_command_executed_has_exit_code() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("command"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let cmds: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => Some((command.clone(), *exit_code, output_preview.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].0, "cargo test");
    assert_eq!(cmds[0].1, Some(0));
    assert_eq!(cmds[0].2, Some("all tests passed".to_string()));
}

#[tokio::test]
async fn content_warning_events_have_messages() {
    let mut rt = Runtime::new();
    rt.register_backend("warn-err", WarningErrorBackend);
    let handle = rt
        .run_streaming("warn-err", make_work_order("warnings"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let warns: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Warning { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(warns, vec!["warn-1", "warn-2"]);
}

#[tokio::test]
async fn content_error_events_have_messages() {
    let mut rt = Runtime::new();
    rt.register_backend("error-only", ErrorOnlyBackend);
    let handle = rt
        .run_streaming("error-only", make_work_order("errors"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let errs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Error { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(errs, vec!["something went wrong"]);
}

#[tokio::test]
async fn content_assistant_message_carries_full_text() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("message"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(msgs, vec!["hello world"]);
}

#[tokio::test]
async fn content_run_started_carries_message() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("started msg"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert_eq!(message, "go");
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn content_run_completed_carries_message() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("completed msg"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    if let AgentEventKind::RunCompleted { message } = &events.last().unwrap().kind {
        assert_eq!(message, "done");
    } else {
        panic!("expected RunCompleted");
    }
}

#[tokio::test]
async fn content_tool_call_input_is_valid_json() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("json input"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for ev in &events {
        if let AgentEventKind::ToolCall { input, .. } = &ev.kind {
            assert!(input.is_object(), "tool call input must be a JSON object");
        }
    }
}

#[tokio::test]
async fn content_tool_result_output_is_valid_json() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("json output"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for ev in &events {
        if let AgentEventKind::ToolResult { output, .. } = &ev.kind {
            assert!(
                output.is_object(),
                "tool result output must be a JSON object"
            );
        }
    }
}

#[tokio::test]
async fn content_nested_tool_has_parent_id() {
    let mut rt = Runtime::new();
    rt.register_backend("nested", NestedToolBackend);
    let handle = rt
        .run_streaming("nested", make_work_order("parent id"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let inner_call = events.iter().find(
        |e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "inner"),
    );
    assert!(inner_call.is_some());
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &inner_call.unwrap().kind
    {
        assert_eq!(parent_tool_use_id, &Some("tc-outer".to_string()));
    }
}

#[tokio::test]
async fn content_all_events_have_timestamp() {
    let mut rt = Runtime::new();
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("timestamps"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for ev in &events {
        // ts is a required field; verify it's not epoch zero
        assert!(ev.ts.timestamp() > 0, "event timestamp must be set");
    }
}

#[tokio::test]
async fn content_mock_backend_event_sequence() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("mock seq"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    let names: Vec<_> = events.iter().map(kind_name).collect();
    assert_eq!(names[0], "run_started");
    assert_eq!(*names.last().unwrap(), "run_completed");
    // Mock emits AssistantMessage events between
    assert!(names.contains(&"assistant_message"));
}

// ===========================================================================
// 3. STREAM LIFECYCLE (15+ tests)
// ===========================================================================

#[tokio::test]
async fn lifecycle_stream_collect_to_vec() {
    let (tx, rx) = mpsc::channel(16);
    let stream = EventStream::new(rx);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn lifecycle_stream_collect_filtered() {
    let (tx, rx) = mpsc::channel(16);
    let stream = EventStream::new(rx);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let filter = EventFilter::exclude_errors();
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
    for ev in &events {
        assert!(!matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[tokio::test]
async fn lifecycle_stream_recv_returns_none_on_close() {
    let (tx, rx) = mpsc::channel(16);
    let mut stream = EventStream::new(rx);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "x".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let first = stream.recv().await;
    assert!(first.is_some());
    let second = stream.recv().await;
    assert!(second.is_none());
}

#[tokio::test]
async fn lifecycle_empty_stream_collects_to_empty_vec() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    let stream = EventStream::new(rx);
    drop(_tx);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn lifecycle_stream_pipe_through_pipeline() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    let stream = EventStream::new(rx_in);

    tx_in
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "ok".into(),
        }))
        .await
        .unwrap();
    tx_in
        .send(make_event(AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        }))
        .await
        .unwrap();
    tx_in
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "ok2".into(),
        }))
        .await
        .unwrap();
    drop(tx_in);

    stream.pipe(&pipeline, tx_out).await;

    let mut out = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        out.push(ev);
    }
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn lifecycle_stream_cancellation_mid_flight() {
    let (tx, rx) = mpsc::channel(16);
    let mut stream = EventStream::new(rx);

    // Send events in background
    tokio::spawn(async move {
        for i in 0..100 {
            if tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("d-{i}"),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Only consume a few then drop
    let mut count = 0;
    for _ in 0..5 {
        if stream.recv().await.is_some() {
            count += 1;
        }
    }
    drop(stream);
    assert!(count <= 5);
}

#[tokio::test]
async fn lifecycle_event_stream_into_inner() {
    let (tx, rx) = mpsc::channel(16);
    let stream = EventStream::new(rx);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "x".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let mut inner = stream.into_inner();
    let ev = inner.recv().await;
    assert!(ev.is_some());
    assert!(inner.recv().await.is_none());
}

#[tokio::test]
async fn lifecycle_recorder_captures_all_events() {
    let recorder = EventRecorder::new();
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    for ev in &events {
        recorder.record(ev);
    }
    assert_eq!(recorder.len(), 3);
    let recorded = recorder.events();
    assert_eq!(recorded.len(), 3);
}

#[tokio::test]
async fn lifecycle_recorder_clear_empties() {
    let recorder = EventRecorder::new();
    recorder.record(&make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }));
    assert!(!recorder.is_empty());
    recorder.clear();
    assert!(recorder.is_empty());
}

#[tokio::test]
async fn lifecycle_stats_track_event_counts() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }));
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "world".into(),
    }));
    stats.observe(&make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    }));
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.count_for("assistant_delta"), 2);
    assert_eq!(stats.error_count(), 1);
}

#[tokio::test]
async fn lifecycle_stats_track_delta_bytes() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(), // 5 bytes
    }));
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "world!".into(), // 6 bytes
    }));
    assert_eq!(stats.total_delta_bytes(), 11);
}

#[tokio::test]
async fn lifecycle_stats_reset() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }));
    assert_eq!(stats.total_events(), 1);
    stats.reset();
    assert_eq!(stats.total_events(), 0);
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.total_delta_bytes(), 0);
}

#[tokio::test]
async fn lifecycle_pipeline_with_transform() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .build();

    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    });
    let result = pipeline.process(ev).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &result.kind {
        assert_eq!(text, "HELLO");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[tokio::test]
async fn lifecycle_pipeline_identity_transform() {
    let transform = EventTransform::identity();
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "unchanged".into(),
    });
    let result = transform.apply(ev);
    if let AgentEventKind::AssistantDelta { text } = &result.kind {
        assert_eq!(text, "unchanged");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[tokio::test]
async fn lifecycle_pipeline_filter_then_transform() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                text.push('!');
            }
            ev
        }))
        .build();

    // Delta passes filter and gets transformed
    let delta = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let result = pipeline.process(delta);
    assert!(result.is_some());
    if let AgentEventKind::AssistantDelta { text } = &result.unwrap().kind {
        assert_eq!(text, "hi!");
    }

    // Error gets filtered out
    let error = make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    assert!(pipeline.process(error).is_none());
}

// ===========================================================================
// 4. INTEGRATION WITH RUNTIME (15+ tests)
// ===========================================================================

#[tokio::test]
async fn runtime_run_streaming_produces_events_and_receipt() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("produce"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert!(!events.is_empty());
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn runtime_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("hash"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn runtime_receipt_trace_matches_streamed_events() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("trace match"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Receipt trace should have at least as many events as streamed
    assert!(!receipt.trace.is_empty());
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn runtime_receipt_contract_version() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("version"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn runtime_unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let result = rt
        .run_streaming("nonexistent", make_work_order("unknown"))
        .await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(
        err,
        abp_runtime::RuntimeError::UnknownBackend { .. }
    ));
}

#[tokio::test]
async fn runtime_error_during_streaming_produces_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    let handle = rt
        .run_streaming("failing", make_work_order("fail"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    // Events should still arrive (RunStarted, Error)
    assert!(!events.is_empty());
    // Receipt should be an error
    assert!(receipt.is_err());
}

#[tokio::test]
async fn runtime_metrics_reflect_successful_run() {
    let rt = Runtime::with_default_backends();
    let snap_before = rt.metrics().snapshot();
    let handle = rt
        .run_streaming("mock", make_work_order("metrics"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
    let snap_after = rt.metrics().snapshot();
    assert_eq!(snap_after.total_runs, snap_before.total_runs + 1);
    assert_eq!(snap_after.successful_runs, snap_before.successful_runs + 1);
}

#[tokio::test]
async fn runtime_metrics_count_events() {
    let rt = Runtime::with_default_backends();
    let snap_before = rt.metrics().snapshot();
    let handle = rt
        .run_streaming("mock", make_work_order("event count"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert!(receipt.is_ok());
    let snap_after = rt.metrics().snapshot();
    assert!(snap_after.total_events >= snap_before.total_events + events.len() as u64);
}

#[tokio::test]
async fn runtime_run_id_is_unique() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", make_work_order("run1"))
        .await
        .unwrap();
    let id1 = h1.run_id;
    let _ = drain_run(h1).await;
    let h2 = rt
        .run_streaming("mock", make_work_order("run2"))
        .await
        .unwrap();
    let id2 = h2.run_id;
    let _ = drain_run(h2).await;
    assert_ne!(id1, id2);
}

#[tokio::test]
async fn runtime_receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", make_work_order("chain1"))
        .await
        .unwrap();
    let _ = drain_run(h1).await;
    let h2 = rt
        .run_streaming("mock", make_work_order("chain2"))
        .await
        .unwrap();
    let _ = drain_run(h2).await;
    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert!(chain_guard.len() >= 2);
}

#[tokio::test]
async fn runtime_stream_pipeline_filters_events() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();
    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("warn-err", WarningErrorBackend);
    let handle = rt
        .run_streaming("warn-err", make_work_order("filter"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    for ev in &events {
        assert!(
            !matches!(ev.kind, AgentEventKind::Error { .. }),
            "errors should have been filtered out"
        );
    }
}

#[tokio::test]
async fn runtime_stream_pipeline_with_recorder() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::new().with_stream_pipeline(pipeline);
    let mut rt = rt;
    rt.register_backend("ordered", OrderedDeltaBackend { count: 3 });
    let handle = rt
        .run_streaming("ordered", make_work_order("record"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    // Recorder captured same events as streamed
    assert_eq!(recorder.len(), events.len());
}

#[tokio::test]
async fn runtime_stream_pipeline_with_stats() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    let mut rt = Runtime::new().with_stream_pipeline(pipeline);
    rt.register_backend("mixed", FullMixedBackend);
    let handle = rt
        .run_streaming("mixed", make_work_order("stats"))
        .await
        .unwrap();
    let (events, _) = drain_run(handle).await;
    assert_eq!(stats.total_events(), events.len() as u64);
    assert!(stats.count_for("assistant_delta") >= 1);
    assert!(stats.count_for("run_started") >= 1);
}

#[tokio::test]
async fn runtime_receipt_work_order_id_matches() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("id match");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn runtime_receipt_backend_identity() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", make_work_order("identity"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn runtime_minimal_backend_produces_minimal_events() {
    let mut rt = Runtime::new();
    rt.register_backend("minimal", MinimalBackend);
    let handle = rt
        .run_streaming("minimal", make_work_order("minimal"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert_eq!(events.len(), 2); // RunStarted + RunCompleted
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn runtime_multiple_sequential_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let handle = rt
            .run_streaming("mock", make_work_order(&format!("run-{i}")))
            .await
            .unwrap();
        let (events, receipt) = drain_run(handle).await;
        assert!(!events.is_empty());
        assert!(receipt.is_ok());
    }
    let snap = rt.metrics().snapshot();
    assert!(snap.total_runs >= 5);
}

#[tokio::test]
async fn runtime_receipt_timing_metadata() {
    let rt = Runtime::with_default_backends();
    let before = Utc::now();
    let handle = rt
        .run_streaming("mock", make_work_order("timing"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let after = Utc::now();
    let receipt = receipt.unwrap();
    assert!(receipt.meta.started_at >= before);
    assert!(receipt.meta.finished_at <= after);
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

// ===========================================================================
// Additional edge-case and cross-cutting tests
// ===========================================================================

#[tokio::test]
async fn edge_filter_by_kind_run_started() {
    let filter = EventFilter::by_kind("run_started");
    assert!(filter.matches(&make_event(AgentEventKind::RunStarted {
        message: "go".into()
    })));
    assert!(!filter.matches(&make_event(AgentEventKind::RunCompleted {
        message: "done".into()
    })));
}

#[tokio::test]
async fn edge_filter_by_kind_file_changed() {
    let filter = EventFilter::by_kind("file_changed");
    assert!(filter.matches(&make_event(AgentEventKind::FileChanged {
        path: "a.rs".into(),
        summary: "mod".into(),
    })));
    assert!(!filter.matches(&make_event(AgentEventKind::Warning {
        message: "w".into()
    })));
}

#[tokio::test]
async fn edge_filter_by_kind_command_executed() {
    let filter = EventFilter::by_kind("command_executed");
    assert!(filter.matches(&make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: None,
    })));
}

#[tokio::test]
async fn edge_event_kind_name_coverage() {
    let cases = vec![
        (
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: "d".into(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "t".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "m".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "r".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "r".into(),
                tool_use_id: None,
                output: serde_json::json!({}),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "f".into(),
                summary: "s".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (
            AgentEventKind::Warning {
                message: "w".into(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
            "error",
        ),
    ];
    for (kind, expected_name) in cases {
        assert_eq!(abp_stream::event_kind_name(&kind), expected_name);
    }
}

#[tokio::test]
async fn edge_stats_kind_counts_map() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }));
    stats.observe(&make_event(AgentEventKind::ToolCall {
        tool_name: "t".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    }));
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }));
    let counts = stats.kind_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("tool_call"), Some(&1));
}
