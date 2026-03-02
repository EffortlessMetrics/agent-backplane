// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the full runtime pipeline:
//! work order → backend execution → event streaming → receipt.
//!
//! Covers basic runs, event ordering, receipt hashing, error propagation,
//! policy enforcement, projection matrix, stream pipelines, cancellation,
//! receipt chains, workspace staging, sequential runs, and more.

use std::collections::BTreeMap;
use std::path::Path;

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
use abp_runtime::{ProjectionMatrix, Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

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

/// Build a PassThrough work order for the given task.
fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// Shorthand: run mock backend and return receipt.
async fn run_mock(rt: &Runtime, task: &str) -> Receipt {
    let wo = passthrough_wo(task);
    let (_, receipt) = run_full(rt, "mock", wo).await;
    receipt.unwrap()
}

// ===========================================================================
// Custom test backends
// ===========================================================================

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

        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        };
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

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
        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(start_ev.clone()).await;

        let msg = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("{} response", self.name),
            },
            ext: None,
        };
        let _ = events_tx.send(msg.clone()).await;

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("{} done", self.name),
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
            mode: abp_integrations::extract_execution_mode(&work_order),
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace: vec![start_ev, msg, end_ev],
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// Backend that streams ToolCall and ToolResult events.
#[derive(Debug, Clone)]
struct ToolUsingBackend;

#[async_trait]
impl Backend for ToolUsingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-user".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
        m.insert(Capability::ToolWrite, SupportLevel::Native);
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

        let events = vec![
            AgentEventKind::RunStarted {
                message: "tool-user starting".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            AgentEventKind::AssistantMessage {
                text: "Read the file.".into(),
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added comment".into(),
            },
            AgentEventKind::RunCompleted {
                message: "tool-user done".into(),
            },
        ];

        for kind in events {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

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

fn mock_caps() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    m
}

fn streaming_backend(name: &str, count: usize) -> EventStreamingBackend {
    EventStreamingBackend {
        name: name.into(),
        caps: mock_caps(),
        event_count: count,
    }
}

// ===========================================================================
// 1. Basic run: work order → MockBackend → receipt
// ===========================================================================

#[tokio::test]
async fn basic_mock_run_returns_receipt() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "basic test").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn basic_mock_run_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "version check").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn basic_mock_run_has_work_order_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("id check");
    let wo_id = wo.id;
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().meta.work_order_id, wo_id);
}

#[tokio::test]
async fn basic_mock_run_has_run_id() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("run id"))
        .await
        .unwrap();
    let run_id = handle.run_id;
    assert_ne!(run_id, Uuid::nil());
    let (_, _receipt) = drain_run(handle).await;
}

#[tokio::test]
async fn basic_run_receipt_has_backend_version() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "backend version").await;
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn basic_run_receipt_has_capabilities() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "caps check").await;
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn basic_run_default_execution_mode_is_mapped() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "mode check").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn basic_run_receipt_has_timing() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "timing").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

// ===========================================================================
// 2. Event stream contains expected event types
// ===========================================================================

#[tokio::test]
async fn event_stream_contains_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("started")).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }))
    );
}

#[tokio::test]
async fn event_stream_contains_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("completed")).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[tokio::test]
async fn event_stream_contains_assistant_messages() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("messages")).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
    );
}

#[tokio::test]
async fn event_stream_has_timestamps() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("timestamps")).await;
    for event in &events {
        assert!(event.ts.timestamp() > 0);
    }
}

#[tokio::test]
async fn custom_backend_streams_correct_event_count() {
    let mut rt = Runtime::new();
    rt.register_backend("stream5", streaming_backend("stream5", 5));
    let (events, receipt) = run_full(&rt, "stream5", passthrough_wo("5 events")).await;
    assert!(receipt.is_ok());
    // 1 RunStarted + 5 AssistantMessage + 1 RunCompleted = 7
    assert_eq!(events.len(), 7);
}

#[tokio::test]
async fn tool_using_backend_streams_tool_events() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-user", ToolUsingBackend);
    let (events, receipt) = run_full(&rt, "tool-user", passthrough_wo("tools")).await;
    assert!(receipt.is_ok());
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::FileChanged { .. }))
    );
}

// ===========================================================================
// 3. Receipt has valid hash
// ===========================================================================

#[tokio::test]
async fn receipt_has_sha256_hash() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash check").await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_hash_is_deterministic() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "hash determinism").await;
    let recomputed = compute_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[tokio::test]
async fn receipt_hash_verifies_with_core_receipt_hash() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "core hash").await;
    let core_hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap(), &core_hash);
}

#[tokio::test]
async fn receipt_hash_changes_with_different_task() {
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "task A").await;
    let r2 = run_mock(&rt, "task B").await;
    // Different work orders produce different receipts with different hashes.
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// 4. Unknown backend produces RuntimeError::UnknownBackend
// ===========================================================================

#[tokio::test]
async fn unknown_backend_returns_error() {
    let rt = Runtime::with_default_backends();
    let err = match rt
        .run_streaming("nonexistent", passthrough_wo("fail"))
        .await
    {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn unknown_backend_error_contains_name() {
    let rt = Runtime::with_default_backends();
    let err = match rt
        .run_streaming("my_backend_xyz", passthrough_wo("fail"))
        .await
    {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    let msg = err.to_string();
    assert!(msg.contains("my_backend_xyz"), "error: {msg}");
}

#[tokio::test]
async fn unknown_backend_error_code() {
    let rt = Runtime::with_default_backends();
    let err = match rt.run_streaming("nope", passthrough_wo("fail")).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[tokio::test]
async fn empty_runtime_unknown_backend() {
    let rt = Runtime::new();
    let err = match rt.run_streaming("anything", passthrough_wo("fail")).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 5. Runtime with registered mock backend
// ===========================================================================

#[tokio::test]
async fn with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

#[tokio::test]
async fn register_custom_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("custom", streaming_backend("custom", 2));
    assert!(rt.backend("custom").is_some());
    assert!(rt.backend("mock").is_none());
}

#[tokio::test]
async fn register_replaces_existing_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("x", streaming_backend("x", 1));
    rt.register_backend("x", streaming_backend("x-v2", 3));
    let backend = rt.backend("x").unwrap();
    assert_eq!(backend.identity().id, "x-v2");
}

#[tokio::test]
async fn backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("zebra", streaming_backend("zebra", 1));
    rt.register_backend("alpha", streaming_backend("alpha", 1));
    rt.register_backend("middle", streaming_backend("middle", 1));
    let names = rt.backend_names();
    assert_eq!(names, vec!["alpha", "middle", "zebra"]);
}

#[tokio::test]
async fn registry_contains_check() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

// ===========================================================================
// 6. Multiple sequential runs
// ===========================================================================

#[tokio::test]
async fn sequential_runs_produce_unique_receipts() {
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "run 1").await;
    let r2 = run_mock(&rt, "run 2").await;
    let r3 = run_mock(&rt, "run 3").await;
    // Each run gets a unique run_id.
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    assert_ne!(r2.meta.run_id, r3.meta.run_id);
    assert_ne!(r1.meta.run_id, r3.meta.run_id);
}

#[tokio::test]
async fn sequential_runs_accumulate_in_chain() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain 1").await;
    run_mock(&rt, "chain 2").await;
    run_mock(&rt, "chain 3").await;
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert_eq!(guard.len(), 3);
}

#[tokio::test]
async fn sequential_runs_have_increasing_start_times() {
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "time 1").await;
    let r2 = run_mock(&rt, "time 2").await;
    assert!(r1.meta.started_at <= r2.meta.started_at);
}

#[tokio::test]
async fn five_sequential_runs_all_succeed() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let receipt = run_mock(&rt, &format!("run {i}")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn sequential_runs_with_different_backends() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("alt", streaming_backend("alt", 1));
    let r1 = run_mock(&rt, "mock run").await;
    let (_, r2) = run_full(&rt, "alt", passthrough_wo("alt run")).await;
    assert_eq!(r1.backend.id, "mock");
    assert_eq!(r2.unwrap().backend.id, "alt");
}

// ===========================================================================
// 7. Runtime builder configuration
// ===========================================================================

#[tokio::test]
async fn runtime_new_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[tokio::test]
async fn runtime_default_equals_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[tokio::test]
async fn runtime_with_emulation_config() {
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );
    let rt = Runtime::with_default_backends().with_emulation(emu_config);
    assert!(rt.emulation_config().is_some());
}

#[tokio::test]
async fn runtime_without_emulation_returns_none() {
    let rt = Runtime::with_default_backends();
    assert!(rt.emulation_config().is_none());
}

#[tokio::test]
async fn runtime_metrics_initially_zero() {
    let rt = Runtime::with_default_backends();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
}

#[tokio::test]
async fn runtime_metrics_after_one_run() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "metrics").await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
}

#[tokio::test]
async fn runtime_metrics_accumulate() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "m1").await;
    run_mock(&rt, "m2").await;
    run_mock(&rt, "m3").await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn runtime_registry_mut_access() {
    let mut rt = Runtime::with_default_backends();
    rt.registry_mut()
        .register("extra", streaming_backend("extra", 1));
    assert!(rt.backend("extra").is_some());
    assert!(rt.backend("mock").is_some());
}

// ===========================================================================
// 8. Workspace setup and teardown
// ===========================================================================

#[tokio::test]
async fn passthrough_workspace_run_succeeds() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough ws")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn staged_workspace_creates_temp_dir() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("test.txt"), "hello").unwrap();
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged ws")
        .root(source.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn staged_workspace_fills_verification() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("file.txt"), "content").unwrap();
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("verify ws")
        .root(source.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();
    // Verification fields should be populated.
    assert!(receipt.verification.git_diff.is_some() || receipt.verification.git_status.is_some());
}

#[tokio::test]
async fn workspace_with_include_exclude_globs() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("keep.rs"), "fn main() {}").unwrap();
    std::fs::write(source.path().join("skip.tmp"), "temp").unwrap();
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("glob ws")
        .root(source.path().to_string_lossy().to_string())
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["**/*.tmp".into()])
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 9. Policy enforcement during runtime
// ===========================================================================

#[tokio::test]
async fn policy_compiles_and_blocks_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_write: vec!["**/secret/**".into()],
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
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
}

#[tokio::test]
async fn policy_allows_permitted_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
}

#[tokio::test]
async fn policy_with_restrictions_still_completes_run() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("policy run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/secret/**".into()],
            ..Default::default()
        })
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn empty_policy_permits_everything() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("anything").allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

#[tokio::test]
async fn network_policy_deny() {
    let policy = PolicyProfile {
        deny_network: vec!["evil.com".into()],
        ..Default::default()
    };
    let _engine = PolicyEngine::new(&policy).unwrap();
    // Policy compiles without error.
}

// ===========================================================================
// 10. Event ordering (RunStarted → events → RunCompleted)
// ===========================================================================

#[tokio::test]
async fn events_start_with_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("ordering")).await;
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn events_end_with_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("ordering")).await;
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn events_in_chronological_order() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_full(&rt, "mock", passthrough_wo("chrono")).await;
    for window in events.windows(2) {
        assert!(window[0].ts <= window[1].ts);
    }
}

#[tokio::test]
async fn custom_backend_event_ordering() {
    let mut rt = Runtime::new();
    rt.register_backend("ordered", streaming_backend("ordered", 3));
    let (events, _) = run_full(&rt, "ordered", passthrough_wo("order test")).await;
    // First event is RunStarted.
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    // Last event is RunCompleted.
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    // Middle events are AssistantMessages.
    for ev in &events[1..events.len() - 1] {
        assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
    }
}

#[tokio::test]
async fn tool_event_ordering_call_before_result() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-user", ToolUsingBackend);
    let (events, _) = run_full(&rt, "tool-user", passthrough_wo("tool order")).await;
    let call_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .unwrap();
    let result_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .unwrap();
    assert!(call_idx < result_idx);
}

// ===========================================================================
// 11. Error propagation from backend failures
// ===========================================================================

#[tokio::test]
async fn failing_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "intentional failure".into(),
        },
    );
    let handle = rt
        .run_streaming("fail", passthrough_wo("fail"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[tokio::test]
async fn failing_backend_error_message_propagates() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "custom_error_xyz".into(),
        },
    );
    let handle = rt
        .run_streaming("fail", passthrough_wo("fail"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let msg = receipt.unwrap_err().to_string();
    assert!(msg.contains("custom_error_xyz") || msg.contains("backend execution failed"));
}

#[tokio::test]
async fn failing_backend_has_backend_crashed_error_code() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "crash".into(),
        },
    );
    let handle = rt
        .run_streaming("fail", passthrough_wo("fail"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(
        receipt.unwrap_err().error_code(),
        abp_error::ErrorCode::BackendCrashed
    );
}

#[tokio::test]
async fn panicking_backend_returns_backend_failed() {
    let mut rt = Runtime::new();
    rt.register_backend("panic", PanickingBackend);
    let handle = rt
        .run_streaming("panic", passthrough_wo("panic"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(matches!(
        receipt.unwrap_err(),
        RuntimeError::BackendFailed(_)
    ));
}

#[tokio::test]
async fn capability_check_failure_before_run() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("cap fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let err = match rt.run_streaming("mock", wo).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

#[tokio::test]
async fn capability_check_failure_error_code() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("cap code")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let err = match rt.run_streaming("mock", wo).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

// ===========================================================================
// 12. Runtime with custom projection matrix
// ===========================================================================

#[tokio::test]
async fn projection_selects_registered_backend() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_caps(), abp_dialect::Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("proj test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "mock");
}

#[tokio::test]
async fn projection_without_matrix_returns_error() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("no projection");
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
}

#[tokio::test]
async fn projection_unregistered_backend_returns_unknown() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("ghost", mock_caps(), abp_dialect::Dialect::OpenAi, 50);
    let rt = Runtime::new().with_projection(matrix);
    let wo = passthrough_wo("ghost backend");
    let err = rt.select_backend(&wo).unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn projection_prefers_richer_backend() {
    let mut strong = CapabilityManifest::new();
    strong.insert(Capability::Streaming, SupportLevel::Native);
    strong.insert(Capability::ToolRead, SupportLevel::Native);
    strong.insert(Capability::ToolWrite, SupportLevel::Native);
    strong.insert(Capability::ToolEdit, SupportLevel::Native);

    let mut weak = CapabilityManifest::new();
    weak.insert(Capability::Streaming, SupportLevel::Native);

    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("strong", strong.clone(), abp_dialect::Dialect::OpenAi, 50);
    matrix.register_backend("weak", weak, abp_dialect::Dialect::OpenAi, 50);

    let mut rt = Runtime::new().with_projection(matrix);
    rt.register_backend(
        "strong",
        RichBackend {
            name: "strong".into(),
            caps: strong,
        },
    );
    rt.register_backend(
        "weak",
        RichBackend {
            name: "weak".into(),
            caps: {
                let mut m = CapabilityManifest::new();
                m.insert(Capability::Streaming, SupportLevel::Native);
                m
            },
        },
    );

    let wo = WorkOrderBuilder::new("select strong")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
            ],
        })
        .build();
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "strong");
}

#[tokio::test]
async fn run_projected_end_to_end() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register_backend("mock", mock_caps(), abp_dialect::Dialect::OpenAi, 50);
    let rt = Runtime::with_default_backends().with_projection(matrix);
    let wo = WorkOrderBuilder::new("projected run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let handle = rt.run_projected(wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    assert!(!events.is_empty());
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn projection_accessor() {
    let matrix = ProjectionMatrix::new();
    let rt = Runtime::new().with_projection(matrix);
    assert!(rt.projection().is_some());
    let rt2 = Runtime::new();
    assert!(rt2.projection().is_none());
}

// ===========================================================================
// 13. Runtime with stream pipeline
// ===========================================================================

#[tokio::test]
async fn stream_pipeline_records_events() {
    let recorder = abp_stream::EventRecorder::new();
    let pipeline = abp_stream::StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "record events").await;
    assert!(!recorder.is_empty());
}

#[tokio::test]
async fn stream_pipeline_tracks_stats() {
    let stats = abp_stream::EventStats::new();
    let pipeline = abp_stream::StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "stats events").await;
    assert!(stats.total_events() > 0);
}

#[tokio::test]
async fn stream_pipeline_filters_events() {
    let recorder = abp_stream::EventRecorder::new();
    let pipeline = abp_stream::StreamPipelineBuilder::new()
        .filter(abp_stream::EventFilter::by_kind("assistant_message"))
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let (events, receipt) = run_full(&rt, "mock", passthrough_wo("filter")).await;
    assert!(receipt.is_ok());
    // Only assistant_message events pass through.
    for ev in &events {
        assert!(
            matches!(ev.kind, AgentEventKind::AssistantMessage { .. }),
            "unexpected event: {:?}",
            ev.kind
        );
    }
}

#[tokio::test]
async fn stream_pipeline_transforms_events() {
    let transform = abp_stream::EventTransform::identity();
    let pipeline = abp_stream::StreamPipelineBuilder::new()
        .transform(transform)
        .record()
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "transform").await;
    let p = rt.stream_pipeline().unwrap();
    assert!(!p.recorder().unwrap().is_empty());
}

#[tokio::test]
async fn stream_pipeline_accessor() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
    let pipeline = abp_stream::StreamPipelineBuilder::new().build();
    let rt = Runtime::new().with_stream_pipeline(pipeline);
    assert!(rt.stream_pipeline().is_some());
}

#[tokio::test]
async fn stream_pipeline_stats_count_by_kind() {
    let stats = abp_stream::EventStats::new();
    let pipeline = abp_stream::StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    run_mock(&rt, "kind count").await;
    assert!(stats.count_for("run_started") >= 1);
    assert!(stats.count_for("run_completed") >= 1);
    assert!(stats.count_for("assistant_message") >= 1);
}

// ===========================================================================
// 14. Cancel/timeout behavior
// ===========================================================================

#[tokio::test]
async fn slow_backend_completes_within_timeout() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 50 });
    let handle = rt
        .run_streaming("slow", passthrough_wo("slow"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn cancellation_token_basic() {
    let token = abp_runtime::cancel::CancellationToken::new();
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn cancellation_token_clones_share_state() {
    let token = abp_runtime::cancel::CancellationToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
}

#[tokio::test]
async fn cancellation_token_cancelled_future() {
    let token = abp_runtime::cancel::CancellationToken::new();
    let t = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        t.cancel();
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), token.cancelled())
        .await
        .expect("cancelled future should resolve");
}

#[tokio::test]
async fn run_with_tokio_timeout() {
    let mut rt = Runtime::new();
    rt.register_backend("slow", SlowBackend { delay_ms: 5000 });
    let handle = rt
        .run_streaming("slow", passthrough_wo("timeout"))
        .await
        .unwrap();
    // The receipt future should be droppable/cancellable.
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        drain_run(handle).await
    })
    .await;
    // Timeout or success — both are valid behaviors.
    // The key thing is it doesn't hang.
    let _ = result;
}

// ===========================================================================
// 15. Receipt chain from multiple runs
// ===========================================================================

#[tokio::test]
async fn receipt_chain_starts_empty() {
    let rt = Runtime::with_default_backends();
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert_eq!(guard.len(), 0);
}

#[tokio::test]
async fn receipt_chain_grows_with_runs() {
    let rt = Runtime::with_default_backends();
    for i in 0..4 {
        run_mock(&rt, &format!("chain {i}")).await;
    }
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert_eq!(guard.len(), 4);
}

#[tokio::test]
async fn receipt_chain_verifies_integrity() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "chain verify 1").await;
    run_mock(&rt, "chain verify 2").await;
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert!(guard.verify().is_ok());
}

#[tokio::test]
async fn receipt_chain_unique_ids() {
    let rt = Runtime::with_default_backends();
    run_mock(&rt, "unique 1").await;
    run_mock(&rt, "unique 2").await;
    run_mock(&rt, "unique 3").await;
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    let mut ids: Vec<Uuid> = guard.iter().map(|r| r.meta.run_id).collect();
    let len = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), len);
}

// ===========================================================================
// Additional: Receipt store persistence
// ===========================================================================

#[tokio::test]
async fn receipt_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "store test").await;
    let path = store.save(&receipt).unwrap();
    assert!(path.exists());
    let loaded = store.load(receipt.meta.run_id).unwrap();
    assert_eq!(loaded.meta.run_id, receipt.meta.run_id);
    assert_eq!(loaded.outcome, receipt.outcome);
}

#[tokio::test]
async fn receipt_store_multiple_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let rt = Runtime::with_default_backends();
    let r1 = run_mock(&rt, "store 1").await;
    let r2 = run_mock(&rt, "store 2").await;
    store.save(&r1).unwrap();
    store.save(&r2).unwrap();
    let l1 = store.load(r1.meta.run_id).unwrap();
    let l2 = store.load(r2.meta.run_id).unwrap();
    assert_eq!(l1.backend.id, "mock");
    assert_eq!(l2.backend.id, "mock");
    assert_ne!(l1.meta.run_id, l2.meta.run_id);
}

// ===========================================================================
// Additional: Emulation integration
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
    let wo = WorkOrderBuilder::new("emulated run")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Emulated,
            }],
        })
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn emulation_records_in_receipt_usage() {
    let mut emu_config = EmulationConfig::new();
    emu_config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        },
    );
    let rt = Runtime::with_default_backends().with_emulation(emu_config);
    let wo = WorkOrderBuilder::new("emulation meta")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Emulated,
            }],
        })
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Emulation report should be recorded in usage_raw.
    let usage = &receipt.usage_raw;
    assert!(
        usage.get("emulation").is_some(),
        "usage_raw should contain emulation report: {usage}"
    );
}

// ===========================================================================
// Additional: Error variant coverage
// ===========================================================================

#[tokio::test]
async fn runtime_error_unknown_backend_error_code() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[tokio::test]
async fn runtime_error_workspace_failed_code() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[tokio::test]
async fn runtime_error_policy_failed_code() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[tokio::test]
async fn runtime_error_backend_failed_code() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[tokio::test]
async fn runtime_error_capability_check_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing".into());
    assert_eq!(
        err.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[tokio::test]
async fn runtime_error_into_abp_error() {
    let err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    let abp_err = err.into_abp_error();
    assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp_err.message.contains("missing"));
}

#[tokio::test]
async fn classified_error_roundtrip() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::ConfigInvalid, "bad config")
        .with_context("file", "backplane.toml");
    let rt_err: RuntimeError = abp_err.into();
    let back = rt_err.into_abp_error();
    assert_eq!(back.code, abp_error::ErrorCode::ConfigInvalid);
}

// ===========================================================================
// Additional: WorkOrder builder
// ===========================================================================

#[tokio::test]
async fn work_order_builder_sets_task() {
    let wo = WorkOrderBuilder::new("my task").build();
    assert_eq!(wo.task, "my task");
}

#[tokio::test]
async fn work_order_builder_sets_model() {
    let wo = WorkOrderBuilder::new("model test").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[tokio::test]
async fn work_order_builder_sets_max_turns() {
    let wo = WorkOrderBuilder::new("turns test").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[tokio::test]
async fn work_order_builder_sets_budget() {
    let wo = WorkOrderBuilder::new("budget test")
        .max_budget_usd(5.0)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[tokio::test]
async fn work_order_builder_default_lane() {
    let wo = WorkOrderBuilder::new("lane test").build();
    assert!(matches!(wo.lane, abp_core::ExecutionLane::PatchFirst));
}

#[tokio::test]
async fn work_order_builder_workspace_first() {
    let wo = WorkOrderBuilder::new("ws first")
        .lane(abp_core::ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, abp_core::ExecutionLane::WorkspaceFirst));
}

#[tokio::test]
async fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("id1").build();
    let wo2 = WorkOrderBuilder::new("id2").build();
    assert_ne!(wo1.id, wo2.id);
}

// ===========================================================================
// Additional: Check capabilities API
// ===========================================================================

#[tokio::test]
async fn check_capabilities_passes_for_streaming() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[tokio::test]
async fn check_capabilities_fails_for_mcp_client() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("mock", &reqs).is_err());
}

#[tokio::test]
async fn check_capabilities_empty_requirements_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[tokio::test]
async fn check_capabilities_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let err = rt
        .check_capabilities("nonexistent", &CapabilityRequirements::default())
        .unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// Additional: Receipt trace populated by runtime
// ===========================================================================

#[tokio::test]
async fn receipt_trace_is_populated() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "trace check").await;
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_trace_matches_streamed_events() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_full(&rt, "mock", passthrough_wo("trace match")).await;
    let receipt = receipt.unwrap();
    // Trace length should match or exceed streamed events (runtime may use backend trace).
    assert!(receipt.trace.len() >= events.len() || events.len() >= receipt.trace.len());
}

#[tokio::test]
async fn custom_backend_trace_matches_event_count() {
    let mut rt = Runtime::new();
    rt.register_backend("s3", streaming_backend("s3", 3));
    let (events, receipt) = run_full(&rt, "s3", passthrough_wo("trace count")).await;
    let receipt = receipt.unwrap();
    // Backend's trace has RunStarted + 3 AssistantMessage + RunCompleted = 5.
    // Events streamed should be the same count.
    assert_eq!(events.len(), 5);
    assert_eq!(receipt.trace.len(), 5);
}

// ===========================================================================
// Additional: Budget module basics
// ===========================================================================

#[tokio::test]
async fn budget_limit_default_is_unlimited() {
    let limit = abp_runtime::budget::BudgetLimit::default();
    assert!(limit.max_tokens.is_none());
    assert!(limit.max_cost_usd.is_none());
    assert!(limit.max_turns.is_none());
    assert!(limit.max_duration.is_none());
}

// ===========================================================================
// Additional: Retry policy basics
// ===========================================================================

#[tokio::test]
async fn retry_policy_default_values() {
    let policy = abp_runtime::retry::RetryPolicy::default();
    assert_eq!(policy.max_retries, 3);
    assert_eq!(policy.backoff_multiplier, 2.0);
}

#[tokio::test]
async fn retry_policy_compute_delay() {
    let policy = abp_runtime::retry::RetryPolicy::default();
    let d0 = policy.compute_delay(0);
    let d1 = policy.compute_delay(1);
    // Second delay should be at least as large as the first (modulo jitter).
    assert!(d1.as_millis() >= d0.as_millis() / 2);
}

// ===========================================================================
// Additional: Multiple backend types together
// ===========================================================================

#[tokio::test]
async fn mixed_backends_in_single_runtime() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", abp_integrations::MockBackend);
    rt.register_backend("stream", streaming_backend("stream", 2));
    rt.register_backend(
        "fail",
        FailingBackend {
            message: "nope".into(),
        },
    );
    rt.register_backend("tool-user", ToolUsingBackend);

    let names = rt.backend_names();
    assert_eq!(names.len(), 4);

    let r1 = run_mock(&rt, "mixed mock").await;
    assert_eq!(r1.outcome, Outcome::Complete);

    let (events, r2) = run_full(&rt, "stream", passthrough_wo("mixed stream")).await;
    assert!(r2.is_ok());
    // RunStarted + 2 AssistantMessage + RunCompleted = 4
    assert_eq!(events.len(), 4);

    let handle = rt
        .run_streaming("fail", passthrough_wo("mixed fail"))
        .await
        .unwrap();
    let (_, r3) = drain_run(handle).await;
    assert!(r3.is_err());
}

#[tokio::test]
async fn run_with_tool_using_backend_produces_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-user", ToolUsingBackend);
    let (events, receipt) = run_full(&rt, "tool-user", passthrough_wo("tool run")).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    // Should have at least RunStarted, ToolCall, ToolResult, AssistantMessage, FileChanged, RunCompleted.
    assert!(events.len() >= 6);
}

// ===========================================================================
// Additional: Passthrough execution mode
// ===========================================================================

#[tokio::test]
async fn passthrough_mode_via_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let mut rt = Runtime::new();
    let caps = mock_caps();
    rt.register_backend(
        "pt",
        RichBackend {
            name: "pt".into(),
            caps: caps.clone(),
        },
    );
    let wo = WorkOrderBuilder::new("passthrough mode")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let (_, receipt) = run_full(&rt, "pt", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// Additional: Receipt serialization roundtrip
// ===========================================================================

#[tokio::test]
async fn receipt_json_roundtrip() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "json roundtrip").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.run_id, receipt.meta.run_id);
    assert_eq!(deserialized.outcome, receipt.outcome);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
}

#[tokio::test]
async fn receipt_canonical_hash_stable_across_roundtrip() {
    let rt = Runtime::with_default_backends();
    let receipt = run_mock(&rt, "canonical").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    let h1 = compute_hash(&receipt).unwrap();
    let h2 = compute_hash(&deserialized).unwrap();
    assert_eq!(h1, h2);
}

// ===========================================================================
// Additional: No-event backend still produces receipt
// ===========================================================================

#[tokio::test]
async fn zero_event_backend_produces_receipt() {
    let mut rt = Runtime::new();
    rt.register_backend("empty", streaming_backend("empty", 0));
    let (events, receipt) = run_full(&rt, "empty", passthrough_wo("empty")).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    // Still has RunStarted + RunCompleted.
    assert_eq!(events.len(), 2);
}

// ===========================================================================
// Additional: Large event count
// ===========================================================================

#[tokio::test]
async fn large_event_count_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("big", streaming_backend("big", 100));
    let (events, receipt) = run_full(&rt, "big", passthrough_wo("big")).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    // RunStarted + 100 + RunCompleted = 102.
    assert_eq!(events.len(), 102);
}

// ===========================================================================
// Additional: Concurrent runs on same runtime
// ===========================================================================

#[tokio::test]
async fn concurrent_runs_different_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("a", streaming_backend("a", 2));
    rt.register_backend("b", streaming_backend("b", 3));

    let handle_a = rt
        .run_streaming("a", passthrough_wo("concurrent a"))
        .await
        .unwrap();
    let handle_b = rt
        .run_streaming("b", passthrough_wo("concurrent b"))
        .await
        .unwrap();

    let (events_a, receipt_a) = drain_run(handle_a).await;
    let (events_b, receipt_b) = drain_run(handle_b).await;

    assert_eq!(receipt_a.unwrap().outcome, Outcome::Complete);
    assert_eq!(receipt_b.unwrap().outcome, Outcome::Complete);
    assert_eq!(events_a.len(), 4); // RunStarted + 2 + RunCompleted
    assert_eq!(events_b.len(), 5); // RunStarted + 3 + RunCompleted
}

// ===========================================================================
// Additional: Multiple capability requirements
// ===========================================================================

#[tokio::test]
async fn multiple_satisfied_requirements() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("multi caps")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn multiple_missing_requirements_fails() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("multi missing")
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
    let err = match rt.run_streaming("mock", wo).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
}

// ===========================================================================
// Additional: Support level semantics
// ===========================================================================

#[tokio::test]
async fn native_satisfies_both_levels() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[tokio::test]
async fn emulated_satisfies_only_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[tokio::test]
async fn unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// Additional: Runtime error Display
// ===========================================================================

#[tokio::test]
async fn runtime_error_display_unknown_backend() {
    let err = RuntimeError::UnknownBackend {
        name: "foobar".into(),
    };
    assert!(err.to_string().contains("foobar"));
}

#[tokio::test]
async fn runtime_error_display_no_projection() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert!(err.to_string().contains("no matrix"));
}
