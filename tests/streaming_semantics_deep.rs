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
//! Deep streaming semantics tests: event ordering, accumulation, filtering,
//! serialization, cancellation, edge cases, and receipt aggregation.
//!
//! 70+ tests covering every facet of ABP streaming event semantics.

use abp_core::aggregate::EventAggregator;
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, ReceiptBuilder, WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
) -> anyhow::Result<Receipt> {
    let finished = Utc::now();
    let receipt = Receipt {
        meta: abp_core::RunMetadata {
            run_id,
            work_order_id: work_order.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms: (finished - started).num_milliseconds().unsigned_abs(),
        },
        backend: BackendIdentity {
            id: "deep-test".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: Default::default(),
        trace,
        artifacts: vec![],
        verification: Default::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
}

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

fn simple_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

// ---------------------------------------------------------------------------
// Custom backends
// ---------------------------------------------------------------------------

/// Backend emitting a full lifecycle: start, deltas, tool call/result,
/// file changed, command executed, warning, error, message, completed.
#[derive(Debug, Clone)]
struct FullLifecycleBackend;

#[async_trait]
impl Backend for FullLifecycleBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "full-lifecycle".into(),
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
                message: "begin".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta { text: "Hel".into() },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta { text: "lo ".into() },
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
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-100".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-100".into()),
                output: json!({"content": "fn main() {}"}),
                is_error: false,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-101".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/lib.rs", "content": "pub fn hello() {}"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-101".into()),
                output: json!({"ok": true}),
                is_error: false,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Created new file".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok".into()),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "lint warning".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantMessage {
                text: "Hello world".into(),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend that emits many deltas for accumulation testing.
#[derive(Debug, Clone)]
struct DeltaAccumulatorBackend {
    fragments: Vec<String>,
}

#[async_trait]
impl Backend for DeltaAccumulatorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "delta-accumulator".into(),
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
        for frag in &self.fragments {
            emit(
                &mut trace,
                &tx,
                AgentEventKind::AssistantDelta { text: frag.clone() },
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend that emits multiple tool calls with results.
#[derive(Debug, Clone)]
struct MultiToolBackend;

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
        for i in 0..5 {
            let tool = match i % 3 {
                0 => "Read",
                1 => "Write",
                _ => "Bash",
            };
            let id = format!("tc-{i}");
            emit(
                &mut trace,
                &tx,
                AgentEventKind::ToolCall {
                    tool_name: tool.into(),
                    tool_use_id: Some(id.clone()),
                    parent_tool_use_id: None,
                    input: json!({"index": i}),
                },
            )
            .await;
            emit(
                &mut trace,
                &tx,
                AgentEventKind::ToolResult {
                    tool_name: tool.into(),
                    tool_use_id: Some(id),
                    output: json!({"result": format!("output-{i}")}),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend emitting only start + completed (minimal valid stream).
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend that emits unicode content.
#[derive(Debug, Clone)]
struct UnicodeBackend;

#[async_trait]
impl Backend for UnicodeBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "unicode".into(),
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
            AgentEventKind::AssistantDelta {
                text: "こんにちは".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: " 🌍🚀".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantDelta {
                text: " مرحبا".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::AssistantMessage {
                text: "Héllo wörld".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::FileChanged {
                path: "src/données.rs".into(),
                summary: "Created file with accented name".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-u1".into()),
                parent_tool_use_id: None,
                input: json!({"content": "日本語テスト"}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-u1".into()),
                output: json!({"ok": true}),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend emitting warnings and errors with messages.
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
                message: "disk space low".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "rate limit approaching".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Error {
                message: "timeout on tool X".into(),
                error_code: None,
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::Warning {
                message: "retrying".into(),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend emitting large payloads.
#[derive(Debug, Clone)]
struct LargePayloadBackend {
    payload_size: usize,
}

#[async_trait]
impl Backend for LargePayloadBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "large-payload".into(),
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
        let large_text = "x".repeat(self.payload_size);
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
            AgentEventKind::AssistantMessage {
                text: large_text.clone(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolCall {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-big".into()),
                parent_tool_use_id: None,
                input: json!({"content": large_text}),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::ToolResult {
                tool_name: "Write".into(),
                tool_use_id: Some("tc-big".into()),
                output: json!({"ok": true}),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend that emits file changed events with various paths.
#[derive(Debug, Clone)]
struct FileChangedBackend;

#[async_trait]
impl Backend for FileChangedBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "file-changed".into(),
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
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Modified entrypoint".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::FileChanged {
                path: "tests/integration.rs".into(),
                summary: "Added integration test".into(),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::FileChanged {
                path: "Cargo.toml".into(),
                summary: "Updated dependencies".into(),
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

/// Backend emitting command executed events with various exit codes.
#[derive(Debug, Clone)]
struct CommandBackend;

#[async_trait]
impl Backend for CommandBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "command".into(),
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
            AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("Compiling...".into()),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(1),
                output_preview: Some("test failed".into()),
            },
        )
        .await;
        emit(
            &mut trace,
            &tx,
            AgentEventKind::CommandExecuted {
                command: "echo done".into(),
                exit_code: Some(0),
                output_preview: None,
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
        build_receipt(run_id, &work_order, trace, started)
    }
}

// ===========================================================================
// 1. Events flow in correct order (RunStarted first, RunCompleted last)
// ===========================================================================

#[tokio::test]
async fn run_started_is_first_event() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("order")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn run_completed_is_last_event() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("order")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    assert!(matches!(
        &events[events.len() - 1].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn run_started_contains_message() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("msg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => assert!(!message.is_empty()),
        other => panic!("expected RunStarted, got {other:?}"),
    }
}

#[tokio::test]
async fn run_completed_contains_message() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("msg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    match &events.last().unwrap().kind {
        AgentEventKind::RunCompleted { message } => assert!(!message.is_empty()),
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[tokio::test]
async fn only_one_run_started_in_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("single")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .count();
    assert_eq!(count, 1, "should have exactly one RunStarted");
}

#[tokio::test]
async fn only_one_run_completed_in_stream() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("single")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .count();
    assert_eq!(count, 1, "should have exactly one RunCompleted");
}

// ===========================================================================
// 2. AssistantDelta events accumulate into AssistantMessage
// ===========================================================================

#[tokio::test]
async fn deltas_accumulate_to_full_text() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "acc",
        DeltaAccumulatorBackend {
            fragments: vec!["Hello".into(), ", ".into(), "world".into(), "!".into()],
        },
    );
    let handle = rt
        .run_streaming("acc", simple_wo("accumulate"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let accumulated: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(accumulated, "Hello, world!");
}

#[tokio::test]
async fn each_delta_preserved_individually() {
    let frags = vec!["a".into(), "b".into(), "c".into()];
    let mut rt = Runtime::new();
    rt.register_backend(
        "acc",
        DeltaAccumulatorBackend {
            fragments: frags.clone(),
        },
    );
    let handle = rt
        .run_streaming("acc", simple_wo("individual"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn empty_delta_text_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "acc",
        DeltaAccumulatorBackend {
            fragments: vec!["".into(), "hello".into(), "".into()],
        },
    );
    let handle = rt
        .run_streaming("acc", simple_wo("empty-delta"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["", "hello", ""]);
}

#[tokio::test]
async fn full_lifecycle_has_assistant_message() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("msg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let messages: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec!["Hello world"]);
}

// ===========================================================================
// 3. ToolCall events contain valid tool names
// ===========================================================================

#[tokio::test]
async fn tool_call_has_non_empty_tool_name() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt.run_streaming("multi", simple_wo("tools")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::ToolCall { tool_name, .. } = &e.kind {
            assert!(!tool_name.is_empty(), "tool_name must not be empty");
        }
    }
}

#[tokio::test]
async fn tool_call_names_match_expected_set() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("tool-names"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let names: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["Read", "Write", "Bash", "Read", "Write"]);
}

#[tokio::test]
async fn tool_call_has_input_payload() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt.run_streaming("multi", simple_wo("input")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::ToolCall { input, .. } = &e.kind {
            assert!(!input.is_null(), "tool call input should not be null");
        }
    }
}

#[tokio::test]
async fn tool_call_has_tool_use_id() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt.run_streaming("multi", simple_wo("ids")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::ToolCall { tool_use_id, .. } = &e.kind {
            assert!(tool_use_id.is_some(), "tool_use_id should be set");
            assert!(!tool_use_id.as_ref().unwrap().is_empty());
        }
    }
}

// ===========================================================================
// 4. ToolResult events reference prior ToolCall IDs
// ===========================================================================

#[tokio::test]
async fn tool_result_references_prior_tool_call_id() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt.run_streaming("multi", simple_wo("refs")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let call_ids: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolCall { tool_use_id, .. } => tool_use_id.clone(),
            _ => None,
        })
        .collect();

    let result_ids: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolResult { tool_use_id, .. } => tool_use_id.clone(),
            _ => None,
        })
        .collect();

    assert_eq!(call_ids, result_ids, "result IDs must match call IDs");
}

#[tokio::test]
async fn tool_result_name_matches_call_name() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("name-match"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut call_map: BTreeMap<String, String> = BTreeMap::new();
    for e in &events {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id: Some(id),
            ..
        } = &e.kind
        {
            call_map.insert(id.clone(), tool_name.clone());
        }
    }
    for e in &events {
        if let AgentEventKind::ToolResult {
            tool_name,
            tool_use_id: Some(id),
            ..
        } = &e.kind
        {
            let expected = call_map
                .get(id)
                .expect("result should reference a known call ID");
            assert_eq!(
                tool_name, expected,
                "ToolResult name must match ToolCall name for id {id}"
            );
        }
    }
}

#[tokio::test]
async fn tool_result_follows_tool_call() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt.run_streaming("multi", simple_wo("order")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut last_seen_call_id: Option<String> = None;
    for e in &events {
        match &e.kind {
            AgentEventKind::ToolCall { tool_use_id, .. } => {
                last_seen_call_id = tool_use_id.clone();
            }
            AgentEventKind::ToolResult { tool_use_id, .. } => {
                assert_eq!(
                    tool_use_id, &last_seen_call_id,
                    "ToolResult should immediately follow its ToolCall"
                );
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn tool_result_is_error_flag_present() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("err-flag"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let results: Vec<bool> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolResult { is_error, .. } => Some(*is_error),
            _ => None,
        })
        .collect();
    assert!(!results.is_empty());
    // All results from MultiToolBackend are success
    assert!(results.iter().all(|e| !e));
}

// ===========================================================================
// 5. FileChanged events have valid path patterns
// ===========================================================================

#[tokio::test]
async fn file_changed_paths_are_non_empty() {
    let mut rt = Runtime::new();
    rt.register_backend("fc", FileChangedBackend);
    let handle = rt.run_streaming("fc", simple_wo("paths")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::FileChanged { path, .. } = &e.kind {
            assert!(!path.is_empty(), "file path must not be empty");
        }
    }
}

#[tokio::test]
async fn file_changed_paths_are_relative() {
    let mut rt = Runtime::new();
    rt.register_backend("fc", FileChangedBackend);
    let handle = rt.run_streaming("fc", simple_wo("relative")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::FileChanged { path, .. } = &e.kind {
            assert!(!path.starts_with('/'), "paths should be relative: {path}");
            assert!(
                !path.starts_with('\\'),
                "paths should not start with backslash: {path}"
            );
        }
    }
}

#[tokio::test]
async fn file_changed_has_summary() {
    let mut rt = Runtime::new();
    rt.register_backend("fc", FileChangedBackend);
    let handle = rt.run_streaming("fc", simple_wo("summary")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::FileChanged { summary, .. } = &e.kind {
            assert!(!summary.is_empty(), "summary must not be empty");
        }
    }
}

#[tokio::test]
async fn file_changed_paths_collected() {
    let mut rt = Runtime::new();
    rt.register_backend("fc", FileChangedBackend);
    let handle = rt.run_streaming("fc", simple_wo("collect")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let paths: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::FileChanged { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        paths,
        vec!["src/main.rs", "tests/integration.rs", "Cargo.toml"]
    );
}

// ===========================================================================
// 6. CommandExecuted events have exit codes
// ===========================================================================

#[tokio::test]
async fn command_executed_has_exit_codes() {
    let mut rt = Runtime::new();
    rt.register_backend("cmd", CommandBackend);
    let handle = rt.run_streaming("cmd", simple_wo("exit")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let exit_codes: Vec<Option<i32>> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::CommandExecuted { exit_code, .. } => Some(*exit_code),
            _ => None,
        })
        .collect();
    assert_eq!(exit_codes, vec![Some(0), Some(1), Some(0)]);
}

#[tokio::test]
async fn command_executed_has_command_string() {
    let mut rt = Runtime::new();
    rt.register_backend("cmd", CommandBackend);
    let handle = rt.run_streaming("cmd", simple_wo("cmds")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        if let AgentEventKind::CommandExecuted { command, .. } = &e.kind {
            assert!(!command.is_empty());
        }
    }
}

#[tokio::test]
async fn command_executed_output_preview_optional() {
    let mut rt = Runtime::new();
    rt.register_backend("cmd", CommandBackend);
    let handle = rt.run_streaming("cmd", simple_wo("preview")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let previews: Vec<Option<&str>> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::CommandExecuted { output_preview, .. } => {
                Some(output_preview.as_deref())
            }
            _ => None,
        })
        .collect();
    // Third command has no preview
    assert_eq!(previews.len(), 3);
    assert!(previews[0].is_some());
    assert!(previews[2].is_none());
}

// ===========================================================================
// 7. Warning/Error events have messages
// ===========================================================================

#[tokio::test]
async fn warning_events_have_messages() {
    let mut rt = Runtime::new();
    rt.register_backend("we", WarningErrorBackend);
    let handle = rt.run_streaming("we", simple_wo("warn")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let warnings: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Warning { message } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 3);
    for msg in &warnings {
        assert!(!msg.is_empty(), "warning message must not be empty");
    }
}

#[tokio::test]
async fn error_events_have_messages() {
    let mut rt = Runtime::new();
    rt.register_backend("we", WarningErrorBackend);
    let handle = rt.run_streaming("we", simple_wo("err")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let errors: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Error { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0], "timeout on tool X");
}

#[tokio::test]
async fn error_does_not_terminate_stream_deep() {
    let mut rt = Runtime::new();
    rt.register_backend("we", WarningErrorBackend);
    let handle = rt.run_streaming("we", simple_wo("continue")).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Events continue after error
    let error_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .unwrap();
    assert!(events.len() > error_idx + 1);
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mixed_warnings_and_errors_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("we", WarningErrorBackend);
    let handle = rt.run_streaming("we", simple_wo("mixed")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let types: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::Warning { .. } => Some("warning"),
            AgentEventKind::Error { .. } => Some("error"),
            _ => None,
        })
        .collect();
    assert_eq!(types, vec!["warning", "warning", "error", "warning"]);
}

// ===========================================================================
// 8. Event timestamps are monotonically increasing
// ===========================================================================

#[tokio::test]
async fn timestamps_non_decreasing_full_lifecycle() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("ts")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "timestamps must be non-decreasing: {} after {}",
            window[1].ts,
            window[0].ts,
        );
    }
}

#[tokio::test]
async fn timestamps_non_decreasing_multi_tool() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("ts-multi"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for window in events.windows(2) {
        assert!(window[1].ts >= window[0].ts);
    }
}

#[test]
fn synthetic_timestamps_monotonic() {
    let base = Utc::now();
    let events: Vec<AgentEvent> = (0..10)
        .map(|i| {
            make_event_at(
                AgentEventKind::AssistantDelta {
                    text: format!("d{i}"),
                },
                base + Duration::milliseconds(i * 10),
            )
        })
        .collect();
    for window in events.windows(2) {
        assert!(window[1].ts > window[0].ts);
    }
}

#[test]
fn equal_timestamps_are_valid() {
    let ts = Utc::now();
    let e1 = make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts);
    let e2 = make_event_at(AgentEventKind::AssistantDelta { text: "b".into() }, ts);
    assert_eq!(e1.ts, e2.ts, "equal timestamps should be accepted");
}

// ===========================================================================
// 9. Events can be serialized to JSONL
// ===========================================================================

#[tokio::test]
async fn events_serialize_to_jsonl() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("jsonl")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut jsonl = String::new();
    for ev in &events {
        let line = serde_json::to_string(ev).expect("event should serialize");
        jsonl.push_str(&line);
        jsonl.push('\n');
    }
    // Each line should be valid JSON
    for line in jsonl.lines() {
        let _: serde_json::Value =
            serde_json::from_str(line).expect("each line should be valid JSON");
    }
}

#[tokio::test]
async fn events_roundtrip_through_json() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("roundtrip"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    for ev in &events {
        let json = serde_json::to_string(ev).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2, "roundtrip should produce identical JSON");
    }
}

#[test]
fn single_event_jsonl_format() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(
        !json.contains('\n'),
        "single JSONL line should not contain newlines"
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "run_started");
    assert_eq!(parsed["message"], "go");
}

#[test]
fn tool_call_event_serialization() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tc-42".into()),
        parent_tool_use_id: None,
        input: json!({"path": "file.rs"}),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "tool_call");
    assert_eq!(parsed["tool_name"], "Read");
    assert_eq!(parsed["tool_use_id"], "tc-42");
}

#[test]
fn file_changed_event_serialization() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "updated".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "file_changed");
    assert_eq!(parsed["path"], "src/main.rs");
}

#[test]
fn command_executed_event_serialization() {
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "command_executed");
    assert_eq!(parsed["exit_code"], 0);
}

// ===========================================================================
// 10. Stream can be interrupted/cancelled
// ===========================================================================

#[tokio::test]
async fn stream_partial_consumption() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(32);

    tokio::spawn(async move {
        for i in 0..100 {
            if tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("d{i}"),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Only consume 5 events then drop receiver
    let mut collected = Vec::new();
    for _ in 0..5 {
        if let Some(ev) = rx.recv().await {
            collected.push(ev);
        }
    }
    drop(rx);
    assert_eq!(collected.len(), 5);
}

#[tokio::test]
async fn sender_detects_closed_receiver() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(4);
    drop(rx);
    let result = tx
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "orphan".into(),
        }))
        .await;
    assert!(result.is_err(), "send should fail when receiver is dropped");
}

#[tokio::test]
async fn abort_handle_cancels_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("cancel")).await.unwrap();
    // Abort the receipt future
    handle.receipt.abort();
    // Even after abort, we should not panic
}

// ===========================================================================
// 11. Empty event stream is valid
// ===========================================================================

#[tokio::test]
async fn empty_stream_produces_valid_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("min", MinimalBackend);
    let handle = rt.run_streaming("min", simple_wo("empty")).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(events.len(), 2); // RunStarted + RunCompleted
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn empty_stream_receipt_trace_matches_events() {
    let mut rt = Runtime::new();
    rt.register_backend("min", MinimalBackend);
    let handle = rt.run_streaming("min", simple_wo("trace")).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.trace.len(), events.len());
}

#[test]
fn event_stream_wrapper_empty() {
    let stream = EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
    assert!(stream.duration().is_none());
}

// ===========================================================================
// 12. Very large event payloads
// ===========================================================================

#[tokio::test]
async fn large_assistant_message_payload() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "big",
        LargePayloadBackend {
            payload_size: 100_000,
        },
    );
    let handle = rt.run_streaming("big", simple_wo("large")).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let msg = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::AssistantMessage { text } => Some(text),
        _ => None,
    });
    assert_eq!(msg.unwrap().len(), 100_000);
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn large_tool_input_payload() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "big",
        LargePayloadBackend {
            payload_size: 50_000,
        },
    );
    let handle = rt
        .run_streaming("big", simple_wo("large-tool"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let tool_call = events
        .iter()
        .find(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    assert!(tool_call.is_some());
    if let AgentEventKind::ToolCall { input, .. } = &tool_call.unwrap().kind {
        let content = input["content"].as_str().unwrap();
        assert_eq!(content.len(), 50_000);
    }
}

#[test]
fn large_event_serializes() {
    let big_text = "A".repeat(200_000);
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: big_text.clone(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    match &back.kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 200_000),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

// ===========================================================================
// 13. Unicode content in events
// ===========================================================================

#[tokio::test]
async fn unicode_deltas_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("uni", UnicodeBackend);
    let handle = rt.run_streaming("uni", simple_wo("unicode")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["こんにちは", " 🌍🚀", " مرحبا"]);
}

#[tokio::test]
async fn unicode_message_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("uni", UnicodeBackend);
    let handle = rt.run_streaming("uni", simple_wo("uni-msg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let msg = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(msg, Some("Héllo wörld"));
}

#[tokio::test]
async fn unicode_file_path_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("uni", UnicodeBackend);
    let handle = rt
        .run_streaming("uni", simple_wo("uni-file"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let path = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::FileChanged { path, .. } => Some(path.as_str()),
        _ => None,
    });
    assert_eq!(path, Some("src/données.rs"));
}

#[test]
fn unicode_event_json_roundtrip() {
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "日本語🎉 العربية".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    match &back.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "日本語🎉 العربية"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[tokio::test]
async fn unicode_tool_input_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("uni", UnicodeBackend);
    let handle = rt
        .run_streaming("uni", simple_wo("uni-tool"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let tc = events
        .iter()
        .find(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }));
    assert!(tc.is_some());
    if let AgentEventKind::ToolCall { input, .. } = &tc.unwrap().kind {
        assert_eq!(input["content"], "日本語テスト");
    }
}

// ===========================================================================
// 14. Event filtering and routing
// ===========================================================================

#[tokio::test]
async fn filter_include_assistant_events() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("filter-inc"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let filter = EventFilter::include_kinds(&["assistant_delta", "assistant_message"]);
    let filtered: Vec<_> = events.iter().filter(|e| filter.matches(e)).collect();
    for e in &filtered {
        assert!(
            matches!(
                &e.kind,
                AgentEventKind::AssistantDelta { .. } | AgentEventKind::AssistantMessage { .. }
            ),
            "only assistant events should pass"
        );
    }
    assert!(!filtered.is_empty());
}

#[tokio::test]
async fn filter_exclude_deltas() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("filter-exc"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let filter = EventFilter::exclude_kinds(&["assistant_delta"]);
    let filtered: Vec<_> = events.iter().filter(|e| filter.matches(e)).collect();
    for e in &filtered {
        assert!(
            !matches!(&e.kind, AgentEventKind::AssistantDelta { .. }),
            "deltas should be excluded"
        );
    }
}

#[tokio::test]
async fn event_stream_by_kind() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("by-kind"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let stream = EventStream::new(events);
    let tool_calls = stream.by_kind("tool_call");
    assert_eq!(tool_calls.len(), 2);
    let warnings = stream.by_kind("warning");
    assert_eq!(warnings.len(), 1);
}

#[tokio::test]
async fn event_stream_first_last_of_kind() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("first-last"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let stream = EventStream::new(events);
    let first_delta = stream.first_of_kind("assistant_delta");
    assert!(first_delta.is_some());
    if let AgentEventKind::AssistantDelta { text } = &first_delta.unwrap().kind {
        assert_eq!(text, "Hel");
    }
    let last_delta = stream.last_of_kind("assistant_delta");
    assert!(last_delta.is_some());
    if let AgentEventKind::AssistantDelta { text } = &last_delta.unwrap().kind {
        assert_eq!(text, "world");
    }
}

#[tokio::test]
async fn event_stream_count_by_kind() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("count")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let stream = EventStream::new(events);
    let counts = stream.count_by_kind();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
    assert_eq!(counts.get("assistant_delta"), Some(&3));
    assert_eq!(counts.get("tool_call"), Some(&2));
    assert_eq!(counts.get("tool_result"), Some(&2));
}

#[tokio::test]
async fn event_stream_filter_with_event_filter() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("combo")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let stream = EventStream::new(events);
    let only_tools = EventFilter::include_kinds(&["tool_call", "tool_result"]);
    let filtered = stream.filter(&only_tools);
    assert_eq!(filtered.len(), 4); // 2 calls + 2 results
}

#[test]
fn filter_empty_include_passes_nothing() {
    let filter = EventFilter::include_kinds(&[]);
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!filter.matches(&ev));
}

#[test]
fn filter_empty_exclude_passes_everything() {
    let filter = EventFilter::exclude_kinds(&[]);
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(filter.matches(&ev));
}

// ===========================================================================
// 15. Event aggregation for receipt trace
// ===========================================================================

#[tokio::test]
async fn aggregator_counts_all_events() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("agg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    assert_eq!(agg.event_count(), events.len());
}

#[tokio::test]
async fn aggregator_tracks_tool_calls() {
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("agg-tools"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    let tools = agg.tool_calls();
    assert_eq!(tools, vec!["Read", "Write", "Bash", "Read", "Write"]);
    assert_eq!(agg.unique_tool_count(), 3);
}

#[tokio::test]
async fn aggregator_text_length() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("text-len"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    // Deltas: "Hel" + "lo " + "world" = 11 chars, Message: "Hello world" = 11 chars
    assert_eq!(agg.text_length(), 22);
}

#[tokio::test]
async fn aggregator_detects_errors() {
    let mut rt = Runtime::new();
    rt.register_backend("we", WarningErrorBackend);
    let handle = rt.run_streaming("we", simple_wo("agg-err")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    assert!(agg.has_errors());
    assert_eq!(agg.error_messages(), vec!["timeout on tool X"]);
}

#[tokio::test]
async fn aggregator_no_errors_when_clean() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("no-err")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    assert!(!agg.has_errors());
    assert!(agg.error_messages().is_empty());
}

#[tokio::test]
async fn aggregator_summary_by_kind() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("summary"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    let summary = agg.summary();
    assert_eq!(summary.total_events, events.len());
    assert!(summary.by_kind.contains_key("run_started"));
    assert!(summary.by_kind.contains_key("run_completed"));
    assert_eq!(summary.tool_calls, 2);
    assert_eq!(summary.unique_tools, 2); // Read, Write
}

#[tokio::test]
async fn aggregator_timestamps() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("ts-agg")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    assert!(agg.first_timestamp().is_some());
    assert!(agg.last_timestamp().is_some());
}

#[test]
fn aggregator_empty_has_no_errors() {
    let agg = EventAggregator::new();
    assert_eq!(agg.event_count(), 0);
    assert!(!agg.has_errors());
    assert!(agg.error_messages().is_empty());
    assert_eq!(agg.text_length(), 0);
    assert!(agg.tool_calls().is_empty());
    assert!(agg.first_timestamp().is_none());
    assert!(agg.last_timestamp().is_none());
    assert!(agg.duration_ms().is_none());
}

#[tokio::test]
async fn run_analytics_from_events() {
    use abp_core::aggregate::RunAnalytics;
    let mut rt = Runtime::new();
    rt.register_backend("multi", MultiToolBackend);
    let handle = rt
        .run_streaming("multi", simple_wo("analytics"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let analytics = RunAnalytics::from_events(&events);
    assert!(analytics.is_successful());
    assert!(analytics.tool_usage_ratio() > 0.0);
    assert!(analytics.average_text_per_event() >= 0.0);
}

// ===========================================================================
// Additional edge case tests
// ===========================================================================

#[tokio::test]
async fn receipt_trace_matches_streamed_event_count() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("trace-match"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn receipt_has_hash_after_streaming() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("hash")).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_outcome_complete_on_success() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("outcome"))
        .await
        .unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn ext_field_none_by_default() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("ext")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;
    for e in &events {
        assert!(e.ext.is_none(), "ext should be None by default");
    }
}

#[test]
fn event_with_ext_field_serializes() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(back.ext.is_some());
    assert!(back.ext.unwrap().contains_key("raw_message"));
}

#[tokio::test]
async fn event_stream_duration_computed() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt
        .run_streaming("full", simple_wo("duration"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    let stream = EventStream::new(events);
    // Duration should exist (may be 0 if events happen within same ms)
    let dur = stream.duration();
    // With multiple events it should at least not panic
    assert!(dur.is_some() || stream.len() < 2);
}

#[test]
fn event_stream_into_iter() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let collected: Vec<_> = stream.into_iter().collect();
    assert_eq!(collected.len(), 2);
}

#[test]
fn event_stream_ref_iter() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let count = (&stream).into_iter().count();
    assert_eq!(count, 2);
    // stream is still accessible after ref iteration
    assert_eq!(stream.len(), 2);
}

#[tokio::test]
async fn concurrent_event_ordering_under_backpressure() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(2); // very small buffer
    let count = 50;

    let sender = tokio::spawn(async move {
        for i in 0..count {
            let _ = tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("bp-{i}"),
                }))
                .await;
        }
    });

    let receiver = tokio::spawn(async move {
        let mut stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let mut collected = Vec::new();
        while let Some(ev) = stream.next().await {
            collected.push(ev);
        }
        collected
    });

    sender.await.unwrap();
    let received = receiver.await.unwrap();
    // Verify ordering preserved
    for (i, ev) in received.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("bp-{i}"));
        }
    }
}

#[test]
fn receipt_builder_with_trace() {
    let events = [
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(events[0].clone())
        .add_trace_event(events[1].clone())
        .add_trace_event(events[2].clone())
        .build();
    assert_eq!(receipt.trace.len(), 3);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::new();
    let result = rt.run_streaming("nonexistent", simple_wo("fail")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn multiple_sequential_runs_independent() {
    let mut rt = Runtime::new();
    rt.register_backend("min", MinimalBackend);

    let handle1 = rt.run_streaming("min", simple_wo("run1")).await.unwrap();
    let (events1, receipt1) = drain_run(handle1).await;
    let r1 = receipt1.unwrap();

    let handle2 = rt.run_streaming("min", simple_wo("run2")).await.unwrap();
    let (events2, receipt2) = drain_run(handle2).await;
    let r2 = receipt2.unwrap();

    assert_eq!(events1.len(), events2.len());
    // Different run IDs
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    // Both have valid hashes
    assert!(r1.receipt_sha256.is_some());
    assert!(r2.receipt_sha256.is_some());
}

#[tokio::test]
async fn aggregation_summary_serializable() {
    let mut rt = Runtime::new();
    rt.register_backend("full", FullLifecycleBackend);
    let handle = rt.run_streaming("full", simple_wo("ser")).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    let summary = agg.summary();
    let json = serde_json::to_string(&summary).unwrap();
    let back: abp_core::aggregate::AggregationSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back, summary);
}
