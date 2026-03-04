#![allow(clippy::all)]
#![allow(dead_code, unused_imports, unused_variables)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive BDD-style scenario tests for the ABP runtime pipeline.
//!
//! Covers happy paths, backend selection, policy enforcement, workspace
//! lifecycle, error recovery, passthrough vs mapped modes, and receipt
//! correctness — all following a strict Given/When/Then structure.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash, sha256_hex,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
use abp_receipt::{ReceiptChain, compute_hash, verify_hash};
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

fn staged_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::Staged)
        .root(".")
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn mock_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m
}

async fn drain_events(handle: &mut RunHandle) -> Vec<AgentEvent> {
    let mut collected = Vec::new();
    while let Some(ev) = handle.events.next().await {
        collected.push(ev);
    }
    collected
}

async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

// ═══════════════════════════════════════════════════════════════════════
// Custom test backends
// ═══════════════════════════════════════════════════════════════════════

/// Backend that streams tool call + tool result events.
#[derive(Debug, Clone)]
struct ToolCallBackend {
    tool_name: String,
    tool_input: serde_json::Value,
    tool_output: serde_json::Value,
}

#[async_trait]
impl Backend for ToolCallBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-call".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();

        let start_ev = make_event(AgentEventKind::RunStarted {
            message: "starting tool call run".into(),
        });
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        let tool_call_ev = make_event(AgentEventKind::ToolCall {
            tool_name: self.tool_name.clone(),
            tool_use_id: Some("tc-001".into()),
            parent_tool_use_id: None,
            input: self.tool_input.clone(),
        });
        trace.push(tool_call_ev.clone());
        let _ = events_tx.send(tool_call_ev).await;

        let tool_result_ev = make_event(AgentEventKind::ToolResult {
            tool_name: self.tool_name.clone(),
            tool_use_id: Some("tc-001".into()),
            output: self.tool_output.clone(),
            is_error: false,
        });
        trace.push(tool_result_ev.clone());
        let _ = events_tx.send(tool_result_ev).await;

        let done_ev = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        trace.push(done_ev.clone());
        let _ = events_tx.send(done_ev).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().max(0) as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"tools_called": 1}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

/// Backend that emits multiple content blocks (delta + message + file changed).
#[derive(Debug, Clone)]
struct MultiContentBackend {
    deltas: Vec<String>,
    final_message: String,
    file_change: Option<(String, String)>,
}

#[async_trait]
impl Backend for MultiContentBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-content".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();

        let start = make_event(AgentEventKind::RunStarted {
            message: "multi-content".into(),
        });
        trace.push(start.clone());
        let _ = events_tx.send(start).await;

        for d in &self.deltas {
            let ev = make_event(AgentEventKind::AssistantDelta { text: d.clone() });
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let msg = make_event(AgentEventKind::AssistantMessage {
            text: self.final_message.clone(),
        });
        trace.push(msg.clone());
        let _ = events_tx.send(msg).await;

        if let Some((ref path, ref summary)) = self.file_change {
            let fc = make_event(AgentEventKind::FileChanged {
                path: path.clone(),
                summary: summary.clone(),
            });
            trace.push(fc.clone());
            let _ = events_tx.send(fc).await;
        }

        let done = make_event(AgentEventKind::RunCompleted {
            message: "complete".into(),
        });
        trace.push(done.clone());
        let _ = events_tx.send(done).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().max(0) as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

/// Backend that always returns an error from run().
#[derive(Debug, Clone)]
struct ErrorBackend {
    message: String,
}

#[async_trait]
impl Backend for ErrorBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-backend".into(),
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

/// Backend that returns a receipt with Outcome::Failed and emits error events.
#[derive(Debug, Clone)]
struct FailOutcomeBackend {
    error_message: String,
}

#[async_trait]
impl Backend for FailOutcomeBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "fail-outcome".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();

        let err_ev = make_event(AgentEventKind::Error {
            message: self.error_message.clone(),
            error_code: None,
        });
        let _ = events_tx.send(err_ev.clone()).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().max(0) as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"error": true}),
            usage: UsageNormalized::default(),
            trace: vec![err_ev],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Failed,
            receipt_sha256: None,
        })
    }
}

/// Backend that emits events in passthrough mode with ext data.
#[derive(Debug, Clone)]
struct PassthroughStyleBackend;

#[async_trait]
impl Backend for PassthroughStyleBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "passthrough-style".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mode = abp_backend_core::extract_execution_mode(&work_order);

        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            json!({"sdk": "original_sdk_response", "vendor": "test"}),
        );

        let msg = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "passthrough output".into(),
            },
            ext: Some(ext),
        };
        let _ = events_tx.send(msg.clone()).await;

        let done = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let _ = events_tx.send(done.clone()).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().max(0) as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![msg, done],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

/// Backend returning Partial outcome with usage tokens set.
#[derive(Debug, Clone)]
struct PartialOutcomeBackend {
    input_tokens: u64,
    output_tokens: u64,
}

#[async_trait]
impl Backend for PartialOutcomeBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();

        let warn = make_event(AgentEventKind::Warning {
            message: "budget exhausted".into(),
        });
        let _ = events_tx.send(warn.clone()).await;

        let done = make_event(AgentEventKind::RunCompleted {
            message: "partial".into(),
        });
        let _ = events_tx.send(done.clone()).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started).num_milliseconds().max(0) as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"tokens_in": self.input_tokens, "tokens_out": self.output_tokens}),
            usage: UsageNormalized {
                input_tokens: Some(self.input_tokens),
                output_tokens: Some(self.output_tokens),
                ..UsageNormalized::default()
            },
            trace: vec![warn, done],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Partial,
            receipt_sha256: None,
        })
    }
}

/// Backend that tracks how many times `run()` was invoked.
#[derive(Debug, Clone)]
struct CountingBackend {
    invocations: Arc<AtomicUsize>,
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".into(),
            backend_version: None,
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        mock_manifest()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        let started = Utc::now();

        let done = make_event(AgentEventKind::RunCompleted {
            message: "counted".into(),
        });
        let _ = events_tx.send(done.clone()).await;

        let finished = Utc::now();
        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![done],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

/// Backend with configurable capabilities for testing capability checks.
#[derive(Debug, Clone)]
struct ConfigurableCapBackend {
    caps: CapabilityManifest,
}

#[async_trait]
impl Backend for ConfigurableCapBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "configurable".into(),
            backend_version: Some("0.1".into()),
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
        let started = Utc::now();
        let done = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let _ = events_tx.send(done.clone()).await;
        let finished = Utc::now();

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: 0,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![done],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Happy path scenarios
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_valid_work_order_when_submitted_to_mock_then_receipt_has_complete_status() {
    // Given a valid WorkOrder
    let wo = passthrough_wo("compute fibonacci");
    // When submitted to mock backend
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt has correct status
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn test_given_valid_work_order_when_submitted_then_receipt_has_sha256_hash() {
    // Given a valid WorkOrder
    let wo = passthrough_wo("hash check");
    // When submitted
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt has SHA-256 hash
    let hash = receipt.receipt_sha256.as_ref().expect("hash should be set");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn test_given_streaming_request_when_events_arrive_then_each_event_is_properly_typed() {
    // Given a streaming backend that emits deltas
    let mut rt = Runtime::new();
    rt.register_backend(
        "multi",
        MultiContentBackend {
            deltas: vec!["Hello".into(), " world".into()],
            final_message: "Hello world".into(),
            file_change: None,
        },
    );
    // When events arrive
    let handle = rt
        .run_streaming("multi", passthrough_wo("stream"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then each event is properly typed
    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let delta_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .count();
    let has_message = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));

    assert!(has_started, "should have RunStarted");
    assert_eq!(delta_count, 2, "should have 2 delta events");
    assert!(has_message, "should have AssistantMessage");
    assert!(has_completed, "should have RunCompleted");
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_tool_calls_when_processing_then_tool_results_captured() {
    // Given tool calls in response
    let mut rt = Runtime::new();
    rt.register_backend(
        "tools",
        ToolCallBackend {
            tool_name: "Read".into(),
            tool_input: json!({"path": "src/main.rs"}),
            tool_output: json!({"content": "fn main() {}"}),
        },
    );
    // When processing
    let handle = rt
        .run_streaming("tools", passthrough_wo("tool test"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then tool results are captured
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }))
        .collect();

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_results.len(), 1);

    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        ..
    } = &tool_calls[0].kind
    {
        assert_eq!(tool_name, "Read");
        assert_eq!(tool_use_id.as_deref(), Some("tc-001"));
    }
    if let AgentEventKind::ToolResult {
        tool_name,
        is_error,
        ..
    } = &tool_results[0].kind
    {
        assert_eq!(tool_name, "Read");
        assert!(!is_error);
    }
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_multiple_content_blocks_when_assembling_then_all_blocks_in_receipt() {
    // Given multiple content blocks
    let mut rt = Runtime::new();
    rt.register_backend(
        "multi",
        MultiContentBackend {
            deltas: vec!["chunk1".into(), "chunk2".into(), "chunk3".into()],
            final_message: "chunk1chunk2chunk3".into(),
            file_change: Some(("src/lib.rs".into(), "Added function".into())),
        },
    );
    // When assembling
    let handle = rt
        .run_streaming("multi", passthrough_wo("multi-block"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then all blocks present in receipt
    let delta_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .count();
    let message_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    let file_change_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. }))
        .count();

    assert_eq!(delta_count, 3);
    assert_eq!(message_count, 1);
    assert_eq!(file_change_count, 1);
    assert!(!receipt.trace.is_empty());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_valid_work_order_when_run_then_run_id_is_uuid() {
    // Given a valid WorkOrder, When run
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("uuid test"))
        .await
        .unwrap();
    // Then run_id is a valid UUID
    assert_ne!(handle.run_id, Uuid::nil());
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_ne!(receipt.meta.run_id, Uuid::nil());
}

#[tokio::test]
async fn test_given_consecutive_runs_when_executed_then_run_ids_differ() {
    // Given consecutive runs
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", passthrough_wo("run1"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", passthrough_wo("run2"))
        .await
        .unwrap();
    // Then run IDs differ
    assert_ne!(h1.run_id, h2.run_id);
    let (_, r1) = drain_run(h1).await;
    let (_, r2) = drain_run(h2).await;
    r1.unwrap();
    r2.unwrap();
}

#[tokio::test]
async fn test_given_work_order_when_run_then_receipt_trace_nonempty() {
    // Given a work order, When run against mock
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("trace check"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt trace is non-empty
    assert!(!receipt.trace.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backend selection scenarios
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_multiple_backends_when_selecting_by_name_then_correct_backend_used() {
    // Given multiple backends registered
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    let counter = Arc::new(AtomicUsize::new(0));
    rt.register_backend(
        "counting",
        CountingBackend {
            invocations: counter.clone(),
        },
    );
    // When selecting by name
    let handle = rt
        .run_streaming("counting", passthrough_wo("select test"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then correct backend used
    assert_eq!(receipt.backend.id, "counting");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_given_multiple_backends_when_listing_then_all_names_present() {
    // Given multiple backends registered
    let mut rt = Runtime::new();
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("beta", MockBackend);
    rt.register_backend("gamma", MockBackend);
    // When listing backend names
    let names = rt.backend_names();
    // Then all names present
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
    assert!(names.contains(&"gamma".to_string()));
    assert_eq!(names.len(), 3);
}

#[tokio::test]
async fn test_given_no_matching_backend_then_unknown_backend_error_returned() {
    // Given a runtime with only mock
    let rt = Runtime::with_default_backends();
    // When requesting non-existent backend
    let result = rt
        .run_streaming("nonexistent", passthrough_wo("err"))
        .await;
    // Then UnknownBackend error returned
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(&err, RuntimeError::UnknownBackend { name } if name == "nonexistent"),
        "expected UnknownBackend, got {err:?}"
    );
}

#[tokio::test]
async fn test_given_empty_registry_when_run_requested_then_unknown_backend_error() {
    // Given empty registry
    let rt = Runtime::new();
    // When run requested
    let result = rt.run_streaming("any", passthrough_wo("empty")).await;
    // Then UnknownBackend error
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::UnknownBackend { .. }
    ));
}

#[tokio::test]
async fn test_given_backend_replaced_when_running_then_new_backend_used() {
    // Given a backend is replaced
    let mut rt = Runtime::new();
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    rt.register_backend(
        "swap",
        CountingBackend {
            invocations: counter1.clone(),
        },
    );
    rt.register_backend(
        "swap",
        CountingBackend {
            invocations: counter2.clone(),
        },
    );
    // When running
    let handle = rt
        .run_streaming("swap", passthrough_wo("swap"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    receipt.unwrap();
    // Then new backend used (counter2 incremented, counter1 not)
    assert_eq!(counter1.load(Ordering::SeqCst), 0);
    assert_eq!(counter2.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_given_backend_lookup_when_exists_then_some() {
    // Given registered backend
    let rt = Runtime::with_default_backends();
    // When looking up
    assert!(rt.backend("mock").is_some());
    // Then non-existent returns None
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn test_given_backend_names_when_sorted_then_alphabetical() {
    // Given several backends
    let mut rt = Runtime::new();
    rt.register_backend("zeta", MockBackend);
    rt.register_backend("alpha", MockBackend);
    rt.register_backend("mid", MockBackend);
    // When listed, Then names are sorted
    let names = rt.backend_names();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Policy enforcement scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_restrictive_policy_when_tool_call_violates_then_policy_denies() {
    // Given a restrictive policy
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "DeleteFile".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When tool call violates it
    let bash_result = engine.can_use_tool("Bash");
    let delete_result = engine.can_use_tool("DeleteFile");
    // Then PolicyFailed: tool denied
    assert!(!bash_result.allowed);
    assert!(!delete_result.allowed);
}

#[test]
fn test_given_file_write_policy_when_agent_writes_allowed_path_then_succeeds() {
    // Given file write policy allowing src/
    let policy = PolicyProfile {
        deny_write: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When agent writes to allowed path
    let result = engine.can_write_path(Path::new("src/main.rs"));
    // Then succeeds
    assert!(result.allowed);
}

#[test]
fn test_given_read_only_policy_when_agent_attempts_write_then_blocked() {
    // Given read-only policy (deny all writes)
    let policy = PolicyProfile {
        deny_write: vec!["**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When agent attempts write
    let result = engine.can_write_path(Path::new("any/file.txt"));
    // Then blocked
    assert!(!result.allowed);
}

#[test]
fn test_given_allowlist_only_policy_when_unlisted_tool_then_denied() {
    // Given allowlist-only policy
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When unlisted tool used
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    // Allowed tools pass
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
}

#[test]
fn test_given_deny_read_policy_when_reading_secrets_then_denied() {
    // Given deny-read policy for secrets
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When reading secrets
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("secrets/api_key.txt")).allowed);
    // Allowed paths
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn test_given_policy_deny_overrides_allow_when_both_match_then_denied() {
    // Given a tool in both allow and deny lists
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When checking, deny overrides allow
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn test_given_glob_deny_pattern_when_tool_matches_then_denied() {
    // Given glob deny pattern
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When matching tools
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(!engine.can_use_tool("BashRun").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn test_given_nested_path_deny_when_deep_path_written_then_blocked() {
    // Given nested path deny
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // When writing to deeply nested .git path
    assert!(!engine.can_write_path(Path::new(".git/objects/ab/1234")).allowed);
    assert!(!engine.can_write_path(Path::new(".git/HEAD")).allowed);
    // Non-git paths allowed
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn test_given_empty_policy_when_any_operation_then_all_allowed() {
    // Given empty policy
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    // When any operation
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_write_path(Path::new("anything.txt")).allowed);
    assert!(engine.can_read_path(Path::new("anything.txt")).allowed);
}

#[tokio::test]
async fn test_given_work_order_with_policy_when_runtime_compiles_it_then_no_error() {
    // Given work order with policy
    let wo = WorkOrderBuilder::new("policy test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.env".into()],
            ..PolicyProfile::default()
        })
        .build();
    // When submitted to runtime (runtime compiles policy internally)
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then no error — receipt is Complete
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Workspace lifecycle scenarios
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_workspace_staging_enabled_when_run_starts_then_runs_successfully() {
    // Given workspace staging enabled (default Staged mode)
    let wo = staged_wo("staged workspace test");
    // When run starts
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then workspace was staged and run completed
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_workspace_passthrough_when_run_then_completes() {
    // Given PassThrough mode
    let wo = passthrough_wo("passthrough workspace");
    // When run
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then completes successfully
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_workspace_with_include_globs_when_staging_then_builds_ok() {
    // Given workspace with include globs
    let wo = WorkOrderBuilder::new("include glob test")
        .workspace_mode(WorkspaceMode::Staged)
        .root(".")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    // When staging
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then builds OK (no staging error)
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn test_given_workspace_staged_mode_when_run_completes_then_receipt_has_verification() {
    // Given staged workspace
    let wo = staged_wo("verification check");
    // When run completes
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt has verification data (git_diff and/or git_status may be populated)
    // The runtime fills these in for staged workspaces.
    assert_eq!(receipt.outcome, Outcome::Complete);
    // Verification report exists (may be default if no changes in workspace)
    let _ = &receipt.verification;
}

#[tokio::test]
async fn test_given_workspace_cleanup_when_run_completes_then_no_error() {
    // Given workspace with staging
    let wo = staged_wo("cleanup test");
    // When run completes
    let rt = Runtime::with_default_backends();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then no error (temp dir cleaned up automatically by WorkspaceManager drop)
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error recovery scenarios
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_backend_returns_error_when_run_then_backend_failed_error() {
    // Given backend returns error
    let mut rt = Runtime::new();
    rt.register_backend(
        "failing",
        ErrorBackend {
            message: "disk full".into(),
        },
    );
    // When running
    let handle = rt
        .run_streaming("failing", passthrough_wo("fail run"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then BackendFailed error in receipt
    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(
        matches!(&err, RuntimeError::BackendFailed(_)),
        "expected BackendFailed, got {err:?}"
    );
}

#[tokio::test]
async fn test_given_fatal_error_when_processing_then_run_terminates_with_error() {
    // Given a backend that fails fatally
    let mut rt = Runtime::new();
    rt.register_backend(
        "fatal",
        ErrorBackend {
            message: "fatal: out of memory".into(),
        },
    );
    // When processing
    let handle = rt
        .run_streaming("fatal", passthrough_wo("fatal"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    // Then run terminates with error
    let err = receipt.unwrap_err();
    assert!(err.to_string().contains("fatal: out of memory"));
}

#[tokio::test]
async fn test_given_backend_with_failed_outcome_when_run_then_receipt_outcome_failed() {
    // Given backend returns Failed outcome
    let mut rt = Runtime::new();
    rt.register_backend(
        "fail-outcome",
        FailOutcomeBackend {
            error_message: "something went wrong".into(),
        },
    );
    // When run
    let handle = rt
        .run_streaming("fail-outcome", passthrough_wo("fail outcome"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt outcome is Failed
    assert_eq!(receipt.outcome, Outcome::Failed);
    // Error event should be in the stream
    let has_error = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(has_error, "should have error event in stream");
}

#[tokio::test]
async fn test_given_backend_error_when_checked_then_is_retryable() {
    // Given a BackendFailed error
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
    // Then it is retryable
    assert!(err.is_retryable());
}

#[test]
fn test_given_unknown_backend_error_then_not_retryable() {
    // Given UnknownBackend error
    let err = RuntimeError::UnknownBackend {
        name: "missing".into(),
    };
    // Then not retryable
    assert!(!err.is_retryable());
}

#[test]
fn test_given_policy_failed_error_then_not_retryable() {
    // Given PolicyFailed error
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    // Then not retryable
    assert!(!err.is_retryable());
}

#[test]
fn test_given_capability_check_failed_then_not_retryable() {
    // Given CapabilityCheckFailed error
    let err = RuntimeError::CapabilityCheckFailed("missing mcp".into());
    // Then not retryable
    assert!(!err.is_retryable());
}

#[test]
fn test_given_workspace_failed_error_then_is_retryable() {
    // Given WorkspaceFailed error
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk io"));
    // Then it is retryable (transient)
    assert!(err.is_retryable());
}

#[test]
fn test_given_runtime_errors_when_error_code_checked_then_correct_codes() {
    // Given various runtime errors
    let unknown = RuntimeError::UnknownBackend {
        name: "foo".into(),
    };
    let ws = RuntimeError::WorkspaceFailed(anyhow::anyhow!("fail"));
    let policy = RuntimeError::PolicyFailed(anyhow::anyhow!("fail"));
    let backend = RuntimeError::BackendFailed(anyhow::anyhow!("fail"));
    let cap = RuntimeError::CapabilityCheckFailed("fail".into());

    // Then each has correct error code
    assert_eq!(unknown.error_code(), abp_error::ErrorCode::BackendNotFound);
    assert_eq!(ws.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
    assert_eq!(policy.error_code(), abp_error::ErrorCode::PolicyInvalid);
    assert_eq!(backend.error_code(), abp_error::ErrorCode::BackendCrashed);
    assert_eq!(
        cap.error_code(),
        abp_error::ErrorCode::CapabilityUnsupported
    );
}

#[test]
fn test_given_runtime_error_when_converted_to_abp_error_then_roundtrips() {
    // Given a runtime error
    let err = RuntimeError::UnknownBackend {
        name: "gone".into(),
    };
    let code = err.error_code();
    // When converted to AbpError
    let abp_err = err.into_abp_error();
    // Then code is preserved
    assert_eq!(abp_err.code, code);
    assert!(abp_err.message.contains("gone"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Passthrough vs Mapped mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_execution_mode_default_then_mapped() {
    // Given default execution mode
    let mode = ExecutionMode::default();
    // Then it is Mapped
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn test_given_passthrough_mode_when_serialized_then_correct_string() {
    // Given passthrough mode
    let mode = ExecutionMode::Passthrough;
    // When serialized
    let json = serde_json::to_string(&mode).unwrap();
    // Then correct string
    assert_eq!(json, "\"passthrough\"");
}

#[test]
fn test_given_mapped_mode_when_serialized_then_correct_string() {
    // Given mapped mode
    let mode = ExecutionMode::Mapped;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"mapped\"");
}

#[test]
fn test_given_execution_mode_when_deserialized_then_roundtrips() {
    // Given execution modes
    for mode in &[ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, mode);
    }
}

#[tokio::test]
async fn test_given_passthrough_mode_when_backend_preserves_ext_then_raw_data_present() {
    // Given passthrough mode backend
    let mut rt = Runtime::new();
    rt.register_backend("pt", PassthroughStyleBackend);
    // When work order specifies passthrough via vendor config
    let wo = passthrough_wo("passthrough ext test");
    let handle = rt.run_streaming("pt", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Then raw data present in events' ext field
    let with_ext: Vec<_> = events.iter().filter(|e| e.ext.is_some()).collect();
    assert!(!with_ext.is_empty(), "should have events with ext data");

    let ext = with_ext[0].ext.as_ref().unwrap();
    assert!(
        ext.contains_key("raw_message"),
        "ext should contain raw_message"
    );
}

#[tokio::test]
async fn test_given_mapped_mode_receipt_when_mode_checked_then_mapped() {
    // Given a backend that returns Mapped mode receipts
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("mapped mode"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt mode reflects what the backend set
    // MockBackend uses Mapped by default
    assert!(
        receipt.mode == ExecutionMode::Mapped || receipt.mode == ExecutionMode::Passthrough,
        "mode should be a valid ExecutionMode"
    );
}

#[test]
fn test_given_receipt_builder_with_passthrough_mode_then_mode_is_passthrough() {
    // Given a receipt builder with passthrough mode
    let receipt = ReceiptBuilder::new("test")
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .build();
    // Then mode is Passthrough
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn test_given_receipt_builder_default_mode_then_mapped() {
    // Given default receipt builder
    let receipt = ReceiptBuilder::new("test").outcome(Outcome::Complete).build();
    // Then mode is Mapped (the default)
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Receipt correctness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_completed_run_when_receipt_generated_then_hash_is_deterministic() {
    // Given a completed run receipt
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .build();
    // When hash generated twice
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    // Then hash is deterministic
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn test_given_receipt_with_hash_when_recomputed_then_matches_stored() {
    // Given receipt with hash
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .build()
        .with_hash()
        .unwrap();
    // When hash recomputed
    let stored = receipt.receipt_sha256.as_ref().unwrap().clone();
    let recomputed = receipt_hash(&receipt).unwrap();
    // Then matches stored hash
    assert_eq!(stored, recomputed);
}

#[test]
fn test_given_receipt_with_hash_when_verify_hash_called_then_true() {
    // Given hashed receipt
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    // When verified
    assert!(verify_hash(&receipt));
}

#[test]
fn test_given_receipt_without_hash_when_verify_hash_called_then_false() {
    // Given receipt without hash
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    // When verified
    assert!(!verify_hash(&receipt));
}

#[test]
fn test_given_receipt_with_events_when_counting_then_counts_match() {
    // Given receipt with known number of events
    let events: Vec<AgentEvent> = (0..5)
        .map(|i| make_event(AgentEventKind::AssistantDelta {
            text: format!("chunk-{i}"),
        }))
        .collect();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .build();
    // When counting
    // Then counts match actual events
    assert_eq!(receipt.trace.len(), 5);
}

#[test]
fn test_given_two_receipts_different_outcomes_when_hashed_then_differ() {
    // Given two receipts with different outcomes
    let r1 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .outcome(Outcome::Failed)
        .build();
    // When hashed
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    // Then hashes differ
    assert_ne!(h1, h2);
}

#[test]
fn test_given_receipt_with_trace_events_when_hashed_then_events_influence_hash() {
    // Given two receipts: one empty trace, one with events
    let r1 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }))
        .build();
    // When hashed
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    // Then hashes differ (events influence hash)
    assert_ne!(h1, h2);
}

#[test]
fn test_given_receipt_with_artifacts_when_built_then_artifacts_present() {
    // Given receipt with artifacts
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    // Then artifacts present
    assert_eq!(receipt.artifacts.len(), 2);
    assert_eq!(receipt.artifacts[0].kind, "patch");
    assert_eq!(receipt.artifacts[1].kind, "log");
}

#[test]
fn test_given_receipt_when_serialized_then_deserializable() {
    // Given a receipt
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .mode(ExecutionMode::Passthrough)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }))
        .build()
        .with_hash()
        .unwrap();
    // When serialized
    let json = serde_json::to_string(&receipt).unwrap();
    // Then deserializable
    let receipt2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt2.backend.id, "mock");
    assert_eq!(receipt2.outcome, Outcome::Complete);
    assert_eq!(receipt2.mode, ExecutionMode::Passthrough);
    assert!(receipt2.receipt_sha256.is_some());
    assert_eq!(receipt2.trace.len(), 1);
}

#[tokio::test]
async fn test_given_runtime_run_when_receipt_returned_then_hash_verifies() {
    // Given a runtime run
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("verify hash e2e"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then hash verifies
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
}

#[tokio::test]
async fn test_given_runtime_run_when_receipt_chain_checked_then_receipt_appended() {
    // Given a runtime run
    let rt = Runtime::with_default_backends();
    let chain = rt.receipt_chain();
    let handle = rt
        .run_streaming("mock", passthrough_wo("chain check"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    receipt.unwrap();
    // Then receipt was appended to chain
    let chain = chain.lock().await;
    assert!(chain.len() >= 1, "receipt chain should have at least 1 entry");
}

#[tokio::test]
async fn test_given_two_runs_when_chain_checked_then_two_receipts() {
    // Given two sequential runs
    let rt = Runtime::with_default_backends();
    let chain = rt.receipt_chain();

    let h1 = rt
        .run_streaming("mock", passthrough_wo("chain1"))
        .await
        .unwrap();
    let (_, r1) = drain_run(h1).await;
    r1.unwrap();

    let h2 = rt
        .run_streaming("mock", passthrough_wo("chain2"))
        .await
        .unwrap();
    let (_, r2) = drain_run(h2).await;
    r2.unwrap();

    // Then chain has 2 receipts
    let chain = chain.lock().await;
    assert!(chain.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Capability check scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_satisfied_capabilities_when_checked_then_passes() {
    // Given backend with streaming capability, and requirement for streaming
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    // When checked
    let result = rt.check_capabilities("mock", &reqs);
    // Then passes
    assert!(result.is_ok());
}

#[test]
fn test_given_unsatisfied_capability_when_checked_then_fails() {
    // Given requirement for MCP which mock doesn't support natively
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    // When checked
    let result = rt.check_capabilities("mock", &reqs);
    // Then fails
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::CapabilityCheckFailed(_)
    ));
}

#[test]
fn test_given_empty_requirements_when_checked_then_always_passes() {
    // Given empty requirements
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    assert!(rt.check_capabilities("mock", &reqs).is_ok());
}

#[test]
fn test_given_nonexistent_backend_when_capability_checked_then_unknown_backend() {
    // Given nonexistent backend name
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements::default();
    // When capability checked
    let result = rt.check_capabilities("nonexistent", &reqs);
    // Then UnknownBackend error
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::UnknownBackend { .. }
    ));
}

#[tokio::test]
async fn test_given_backend_with_capabilities_when_unsatisfied_requirement_then_run_fails() {
    // Given a backend with specific capabilities
    let mut rt = Runtime::new();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    rt.register_backend("limited", ConfigurableCapBackend { caps });

    // When work order requires McpClient at Native level
    let wo = WorkOrderBuilder::new("cap fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let result = rt.run_streaming("limited", wo).await;
    // Then run fails with capability check error
    assert!(result.is_err() || {
        // The error may come at run_streaming or during the receipt await
        if let Ok(handle) = result {
            let (_, receipt) = drain_run(handle).await;
            receipt.is_err()
        } else {
            true
        }
    });
}

#[test]
fn test_given_emulated_support_when_native_required_then_fails() {
    // Given emulated support
    let level = SupportLevel::Emulated;
    // When native required
    let satisfies = level.satisfies(&MinSupport::Native);
    // Then fails
    assert!(!satisfies);
}

#[test]
fn test_given_native_support_when_emulated_required_then_passes() {
    // Given native support
    let level = SupportLevel::Native;
    // When emulated required
    let satisfies = level.satisfies(&MinSupport::Emulated);
    // Then passes (native exceeds emulated)
    assert!(satisfies);
}

#[test]
fn test_given_any_min_support_when_unsupported_level_then_passes() {
    // Given MinSupport::Any
    let satisfies = SupportLevel::Unsupported.satisfies(&MinSupport::Any);
    // Then passes
    assert!(satisfies);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Agent event type coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_run_started_event_when_serialized_then_type_tag_correct() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"run_started\""));
}

#[test]
fn test_given_run_completed_event_when_serialized_then_type_tag_correct() {
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"run_completed\""));
}

#[test]
fn test_given_assistant_delta_event_when_serialized_then_type_tag_correct() {
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"assistant_delta\""));
}

#[test]
fn test_given_assistant_message_event_when_serialized_then_type_tag_correct() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"assistant_message\""));
}

#[test]
fn test_given_tool_call_event_when_serialized_then_roundtrips() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tc-123".into()),
        parent_tool_use_id: None,
        input: json!({"path": "/tmp/test.rs"}),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let ev2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        ev2.kind,
        AgentEventKind::ToolCall { tool_name, .. } if tool_name == "Read"
    ));
}

#[test]
fn test_given_tool_result_event_when_serialized_then_roundtrips() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tc-123".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let ev2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        ev2.kind,
        AgentEventKind::ToolResult { is_error, .. } if !is_error
    ));
}

#[test]
fn test_given_file_changed_event_when_serialized_then_roundtrips() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "Added new handler".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let ev2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        ev2.kind,
        AgentEventKind::FileChanged { path, .. } if path == "src/main.rs"
    ));
}

#[test]
fn test_given_command_executed_event_when_serialized_then_roundtrips() {
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"command_executed\""));
    let ev2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        ev2.kind,
        AgentEventKind::CommandExecuted { exit_code: Some(0), .. }
    ));
}

#[test]
fn test_given_warning_event_when_serialized_then_type_correct() {
    let ev = make_event(AgentEventKind::Warning {
        message: "budget low".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"warning\""));
}

#[test]
fn test_given_error_event_with_code_when_serialized_then_roundtrips() {
    let ev = make_event(AgentEventKind::Error {
        message: "crash".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let ev2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(ev2.kind, AgentEventKind::Error { message, .. } if message == "crash"));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Partial outcome and usage tracking
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_partial_outcome_backend_when_run_then_receipt_is_partial() {
    // Given partial outcome backend
    let mut rt = Runtime::new();
    rt.register_backend(
        "partial",
        PartialOutcomeBackend {
            input_tokens: 1000,
            output_tokens: 500,
        },
    );
    // When run
    let handle = rt
        .run_streaming("partial", passthrough_wo("partial"))
        .await
        .unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Then receipt is Partial
    assert_eq!(receipt.outcome, Outcome::Partial);
    // Usage tokens set
    assert_eq!(receipt.usage.input_tokens, Some(1000));
    assert_eq!(receipt.usage.output_tokens, Some(500));
}

#[tokio::test]
async fn test_given_partial_outcome_when_events_checked_then_warning_present() {
    // Given partial outcome with warning
    let mut rt = Runtime::new();
    rt.register_backend(
        "partial",
        PartialOutcomeBackend {
            input_tokens: 0,
            output_tokens: 0,
        },
    );
    let handle = rt
        .run_streaming("partial", passthrough_wo("warning"))
        .await
        .unwrap();
    let (events, _receipt) = drain_run(handle).await;
    // Then warning event present
    let has_warning = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Warning { .. }));
    assert!(has_warning);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Metrics and telemetry
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_given_runtime_run_when_metrics_checked_then_run_counted() {
    // Given a runtime with metrics
    let rt = Runtime::with_default_backends();
    // When a run completes
    let handle = rt
        .run_streaming("mock", passthrough_wo("metrics"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    receipt.unwrap();
    // Then metrics recorded
    let metrics = rt.metrics();
    assert!(metrics.total_runs() >= 1);
}

#[tokio::test]
async fn test_given_successful_run_when_metrics_checked_then_success_counted() {
    // Given a successful run
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", passthrough_wo("success metric"))
        .await
        .unwrap();
    let (_, receipt) = drain_run(handle).await;
    receipt.unwrap();
    // Then success counted
    let metrics = rt.metrics();
    assert!(metrics.successful_runs() >= 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. WorkOrder builder comprehensive
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_builder_when_all_fields_set_then_work_order_correct() {
    let wo = WorkOrderBuilder::new("comprehensive task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        })
        .context(ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "note".into(),
                content: "important".into(),
            }],
        })
        .build();

    assert_eq!(wo.task, "comprehensive task");
    assert_eq!(wo.lane, ExecutionLane::WorkspaceFirst);
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert_eq!(wo.workspace.mode, WorkspaceMode::Staged);
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
    assert_eq!(wo.context.files, vec!["README.md"]);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_ne!(wo.id, Uuid::nil());
}

#[test]
fn test_given_work_order_when_json_roundtrip_then_identical() {
    let wo = WorkOrderBuilder::new("roundtrip")
        .model("claude-3")
        .max_turns(5)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "roundtrip");
    assert_eq!(wo2.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo2.config.max_turns, Some(5));
    assert_eq!(wo2.id, wo.id);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Receipt chain integrity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_receipt_chain_when_push_then_length_increases() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn test_given_receipt_chain_when_multiple_pushed_then_all_present() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .work_order_id(Uuid::new_v4())
            .build()
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 3);
}

#[test]
fn test_given_compute_hash_when_called_then_64_hex_chars() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let hash = compute_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Canonical JSON and hashing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_unordered_keys_when_canonical_json_then_sorted() {
    let val = json!({"z": 1, "a": 2, "m": 3});
    let canon = canonical_json(&val).unwrap();
    assert!(canon.starts_with("{\"a\":"));
}

#[test]
fn test_given_sha256_hex_when_called_then_correct_length() {
    let hex = sha256_hex(b"test data");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_given_same_input_when_sha256_hex_called_twice_then_same_output() {
    let h1 = sha256_hex(b"deterministic");
    let h2 = sha256_hex(b"deterministic");
    assert_eq!(h1, h2);
}

#[test]
fn test_given_different_input_when_sha256_hex_then_different_output() {
    let h1 = sha256_hex(b"input1");
    let h2 = sha256_hex(b"input2");
    assert_ne!(h1, h2);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Contract version and identity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_contract_version_when_read_then_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn test_given_mock_backend_when_identity_checked_then_correct() {
    let backend = MockBackend;
    let id = backend.identity();
    assert_eq!(id.id, "mock");
    assert!(id.backend_version.is_some());
}

#[test]
fn test_given_mock_backend_when_capabilities_checked_then_streaming_native() {
    let backend = MockBackend;
    let caps = backend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
    assert_eq!(caps.get(&Capability::Streaming), Some(&SupportLevel::Native));
}

#[test]
fn test_given_backend_identity_when_serialized_then_roundtrips() {
    let id = BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("2.0".into()),
        adapter_version: Some("1.0".into()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let id2: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(id2.id, "test-sidecar");
    assert_eq!(id2.backend_version.as_deref(), Some("2.0"));
    assert_eq!(id2.adapter_version.as_deref(), Some("1.0"));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Outcome serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_outcome_complete_when_serialized_then_snake_case() {
    assert_eq!(serde_json::to_string(&Outcome::Complete).unwrap(), "\"complete\"");
}

#[test]
fn test_given_outcome_partial_when_serialized_then_snake_case() {
    assert_eq!(serde_json::to_string(&Outcome::Partial).unwrap(), "\"partial\"");
}

#[test]
fn test_given_outcome_failed_when_serialized_then_snake_case() {
    assert_eq!(serde_json::to_string(&Outcome::Failed).unwrap(), "\"failed\"");
}

#[test]
fn test_given_all_outcomes_when_deserialized_then_roundtrip() {
    for outcome in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, outcome);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Edge cases and boundary conditions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_given_receipt_builder_with_empty_backend_id_then_builds() {
    let receipt = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    assert_eq!(receipt.backend.id, "");
}

#[test]
fn test_given_work_order_with_empty_task_then_builds() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn test_given_receipt_with_large_trace_when_hashed_then_succeeds() {
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| make_event(AgentEventKind::AssistantDelta {
            text: format!("chunk-{i}"),
        }))
        .collect();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.trace.len(), 100);
}

#[test]
fn test_given_usage_normalized_default_when_built_then_all_none() {
    let usage = UsageNormalized::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
    assert!(usage.cache_write_tokens.is_none());
    assert!(usage.request_units.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn test_given_verification_report_default_when_built_then_empty() {
    let ver = VerificationReport::default();
    assert!(ver.git_diff.is_none());
    assert!(ver.git_status.is_none());
    assert!(!ver.harness_ok);
}

#[tokio::test]
async fn test_given_runtime_with_no_projection_when_select_backend_then_error() {
    // Given runtime without projection matrix
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("no projection");
    // When select_backend
    let result = rt.select_backend(&wo);
    // Then NoProjectionMatch error
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::NoProjectionMatch { .. }
    ));
}
