#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive end-to-end runtime tests covering the full pipeline:
//! WorkOrder creation → Backend selection → Event streaming → Receipt generation.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, CONTRACT_VERSION,
};
use abp_integrations::Backend;
use abp_receipt::{compute_hash, verify_hash};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter};
use abp_runtime::pipeline::{Pipeline, PolicyStage, ValidationStage};
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = passthrough_wo(task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

fn make_temp_workspace() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn hello() {}").unwrap();
    temp
}

// ===========================================================================
// Custom test backends
// ===========================================================================

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
        let started = Utc::now();
        let mut trace = Vec::new();

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "multi-event starting".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: format!("message-{i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "main.rs"}),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

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
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({"note": "multi-event"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        receipt.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

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

/// Backend that emits events before failing.
#[derive(Debug, Clone)]
struct PartialFailBackend;

#[async_trait]
impl Backend for PartialFailBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial-fail".into(),
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
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting before crash".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "about to fail".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;

        anyhow::bail!("partial failure after events")
    }
}

/// Backend that tracks how many times it was invoked.
#[derive(Debug)]
struct CountingBackend {
    count: AtomicUsize,
}

impl Clone for CountingBackend {
    fn clone(&self) -> Self {
        Self {
            count: AtomicUsize::new(self.count.load(Ordering::Relaxed)),
        }
    }
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
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
        self.count.fetch_add(1, Ordering::Relaxed);
        let started = Utc::now();
        let mut trace = Vec::new();

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "counting".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let finished = Utc::now();
        Ok(Receipt {
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
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// Backend with rich capabilities for capability checks.
#[derive(Debug, Clone)]
struct RichCapabilityBackend;

#[async_trait]
impl Backend for RichCapabilityBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "rich".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("1.0".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
        m.insert(Capability::ToolWrite, SupportLevel::Native);
        m.insert(Capability::ToolEdit, SupportLevel::Native);
        m.insert(Capability::ToolBash, SupportLevel::Native);
        m.insert(Capability::McpClient, SupportLevel::Emulated);
        m.insert(Capability::StructuredOutputJsonSchema, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = Utc::now();
        let mut trace = Vec::new();
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "rich backend".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        trace.push(ev.clone());
        let _ = events_tx.send(ev).await;

        let finished = Utc::now();
        Ok(Receipt {
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
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ===========================================================================
// Module 1: Full pipeline — WorkOrder → Backend → Events → Receipt
// ===========================================================================

mod full_pipeline {
    use super::*;

    #[tokio::test]
    async fn submit_and_receive_complete_receipt() {
        let rt = Runtime::with_default_backends();
        let (events, receipt) = run_mock(&rt, "full pipeline test").await;

        assert!(!events.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn receipt_has_valid_sha256_hash() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "hash validation").await;

        assert!(receipt.receipt_sha256.is_some());
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_hash_matches_recomputed() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "hash recompute").await;

        let stored = receipt.receipt_sha256.as_ref().unwrap().clone();
        let recomputed = compute_hash(&receipt).unwrap();
        assert_eq!(stored, recomputed);
    }

    #[tokio::test]
    async fn receipt_backend_identity_is_correct() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "identity check").await;

        assert_eq!(receipt.backend.id, "mock");
        assert!(receipt.backend.backend_version.is_some());
    }

    #[tokio::test]
    async fn receipt_timing_metadata_is_populated() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "timing check").await;

        assert!(receipt.meta.started_at <= receipt.meta.finished_at);
        assert_ne!(receipt.meta.work_order_id, Uuid::nil());
        assert_ne!(receipt.meta.run_id, Uuid::nil());
    }

    #[tokio::test]
    async fn receipt_contains_trace_events() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "trace check").await;

        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn receipt_mode_defaults_to_mapped() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "mode check").await;

        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn receipt_capabilities_include_streaming() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "caps check").await;

        assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    }

    #[tokio::test]
    async fn work_order_id_propagated_to_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("id propagation");
        let wo_id = wo.id;
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn run_id_is_unique_per_run() {
        let rt = Runtime::with_default_backends();

        let wo1 = passthrough_wo("run 1");
        let handle1 = rt.run_streaming("mock", wo1).await.unwrap();
        let run_id_1 = handle1.run_id;

        let wo2 = passthrough_wo("run 2");
        let handle2 = rt.run_streaming("mock", wo2).await.unwrap();
        let run_id_2 = handle2.run_id;

        assert_ne!(run_id_1, run_id_2);

        // Drain both to avoid leaks
        let _ = drain_run(handle1).await;
        let _ = drain_run(handle2).await;
    }
}

// ===========================================================================
// Module 2: MockBackend e2e flow
// ===========================================================================

mod mock_backend_flow {
    use super::*;

    #[tokio::test]
    async fn mock_emits_run_started_and_completed() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_mock(&rt, "lifecycle events").await;

        let has_started = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
        let has_completed = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));

        assert!(has_started);
        assert!(has_completed);
    }

    #[tokio::test]
    async fn mock_emits_assistant_messages() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_mock(&rt, "messages check").await;

        let messages: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::AssistantMessage { text } => Some(text.clone()),
                _ => None,
            })
            .collect();

        assert!(!messages.is_empty());
        for msg in &messages {
            assert!(!msg.is_empty());
        }
    }

    #[tokio::test]
    async fn mock_events_are_chronological() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_mock(&rt, "chronological check").await;

        for window in events.windows(2) {
            assert!(window[1].ts >= window[0].ts);
        }
    }

    #[tokio::test]
    async fn mock_first_event_is_run_started() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_mock(&rt, "first event check").await;

        assert!(matches!(
            &events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ));
    }

    #[tokio::test]
    async fn mock_last_event_is_run_completed() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_mock(&rt, "last event check").await;

        assert!(matches!(
            &events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn mock_receipt_usage_contains_note() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "usage check").await;

        assert!(receipt.usage_raw.get("note").is_some());
    }

    #[tokio::test]
    async fn mock_receipt_harness_ok() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "harness check").await;

        assert!(receipt.verification.harness_ok);
    }

    #[tokio::test]
    async fn mock_receipt_usage_normalized_zeros() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "normalized usage").await;

        assert_eq!(receipt.usage.input_tokens, Some(0));
        assert_eq!(receipt.usage.output_tokens, Some(0));
    }
}

// ===========================================================================
// Module 3: Policy enforcement in runtime
// ===========================================================================

mod policy_enforcement {
    use super::*;

    #[tokio::test]
    async fn empty_policy_allows_run() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("empty policy test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(PolicyProfile::default())
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn deny_read_policy_compiles_successfully() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            deny_read: vec!["**/*.secret".to_string(), "**/.env".to_string()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("deny read policy")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn deny_write_policy_compiles_successfully() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            deny_write: vec!["**/protected/**".to_string()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("deny write policy")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn allowed_tools_policy_compiles() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".to_string(), "Write".to_string()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("allowed tools")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn disallowed_tools_policy_compiles() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".to_string()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("disallowed tools")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn complex_policy_with_all_fields() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".to_string()],
            disallowed_tools: vec!["Bash".to_string()],
            deny_read: vec!["**/*.key".to_string()],
            deny_write: vec!["**/immutable/**".to_string()],
            allow_network: vec!["api.example.com".to_string()],
            deny_network: vec!["evil.com".to_string()],
            require_approval_for: vec!["Deploy".to_string()],
        };
        let wo = WorkOrderBuilder::new("complex policy")
            .workspace_mode(WorkspaceMode::PassThrough)
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn pipeline_validation_rejects_empty_task() {
        let pipeline = Pipeline::new().stage(ValidationStage);
        let mut wo = WorkOrderBuilder::new("ok")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        wo.task = "".into();
        let result = pipeline.execute(&mut wo).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn pipeline_validation_accepts_valid_task() {
        let pipeline = Pipeline::new().stage(ValidationStage);
        let mut wo = passthrough_wo("valid task");
        let result = pipeline.execute(&mut wo).await;
        assert!(result.is_ok());
    }
}

// ===========================================================================
// Module 4: Workspace staging in runtime
// ===========================================================================

mod workspace_staging {
    use super::*;

    #[tokio::test]
    async fn staged_workspace_completes() {
        let temp = make_temp_workspace();
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("staged test")
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
    async fn passthrough_workspace_completes() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("passthrough workspace")
            .workspace_mode(WorkspaceMode::PassThrough)
            .root(".")
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_excludes_git_dir() {
        let temp = tempfile::tempdir().unwrap();
        let git_dir = temp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(temp.path().join("code.rs"), "fn main() {}").unwrap();

        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("git exclude test")
            .workspace_mode(WorkspaceMode::Staged)
            .root(temp.path().to_str().unwrap())
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_with_include_globs() {
        let temp = make_temp_workspace();
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("include globs")
            .workspace_mode(WorkspaceMode::Staged)
            .root(temp.path().to_str().unwrap())
            .include(vec!["**/*.rs".to_string()])
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_with_exclude_globs() {
        let temp = make_temp_workspace();
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("exclude globs")
            .workspace_mode(WorkspaceMode::Staged)
            .root(temp.path().to_str().unwrap())
            .exclude(vec!["**/lib.rs".to_string()])
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_with_policy() {
        let temp = make_temp_workspace();
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            deny_write: vec!["**/protected/**".to_string()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("workspace + policy")
            .workspace_mode(WorkspaceMode::Staged)
            .root(temp.path().to_str().unwrap())
            .policy(policy)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn workspace_first_lane_completes() {
        let temp = make_temp_workspace();
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("workspace first lane")
            .lane(ExecutionLane::WorkspaceFirst)
            .workspace_mode(WorkspaceMode::Staged)
            .root(temp.path().to_str().unwrap())
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }
}

// ===========================================================================
// Module 5: Multiple sequential runs
// ===========================================================================

mod sequential_runs {
    use super::*;

    #[tokio::test]
    async fn two_sequential_runs_produce_different_run_ids() {
        let rt = Runtime::with_default_backends();
        let (_, r1) = run_mock(&rt, "seq run 1").await;
        let (_, r2) = run_mock(&rt, "seq run 2").await;

        assert_ne!(r1.meta.run_id, r2.meta.run_id);
    }

    #[tokio::test]
    async fn receipt_chain_grows_across_runs() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        assert_eq!(chain.lock().await.len(), 0);
        run_mock(&rt, "chain 1").await;
        assert_eq!(chain.lock().await.len(), 1);
        run_mock(&rt, "chain 2").await;
        assert_eq!(chain.lock().await.len(), 2);
        run_mock(&rt, "chain 3").await;
        assert_eq!(chain.lock().await.len(), 3);
    }

    #[tokio::test]
    async fn five_sequential_runs_all_complete() {
        let rt = Runtime::with_default_backends();
        for i in 0..5 {
            let (_, receipt) = run_mock(&rt, &format!("seq {i}")).await;
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn metrics_accumulate_across_runs() {
        let rt = Runtime::with_default_backends();

        run_mock(&rt, "metrics 1").await;
        run_mock(&rt, "metrics 2").await;
        run_mock(&rt, "metrics 3").await;

        let snap = rt.metrics().snapshot();
        assert!(snap.total_runs >= 3);
        assert!(snap.successful_runs >= 3);
        assert_eq!(snap.failed_runs, 0);
        assert!(snap.total_events > 0);
    }

    #[tokio::test]
    async fn different_backends_sequential() {
        let mut rt = Runtime::with_default_backends();
        rt.register_backend("multi", MultiEventBackend { event_count: 2 });

        let wo1 = passthrough_wo("mock run");
        let h1 = rt.run_streaming("mock", wo1).await.unwrap();
        let (_, r1) = drain_run(h1).await;
        assert_eq!(r1.unwrap().backend.id, "mock");

        let wo2 = passthrough_wo("multi run");
        let h2 = rt.run_streaming("multi", wo2).await.unwrap();
        let (_, r2) = drain_run(h2).await;
        assert_eq!(r2.unwrap().backend.id, "multi-event");
    }

    #[tokio::test]
    async fn receipt_chain_contains_correct_work_order_ids() {
        let rt = Runtime::with_default_backends();
        let wo1 = passthrough_wo("wo1");
        let wo1_id = wo1.id;
        let h = rt.run_streaming("mock", wo1).await.unwrap();
        let _ = drain_run(h).await;

        let wo2 = passthrough_wo("wo2");
        let wo2_id = wo2.id;
        let h = rt.run_streaming("mock", wo2).await.unwrap();
        let _ = drain_run(h).await;

        let chain = rt.receipt_chain();
        let guard = chain.lock().await;
        let receipts: Vec<_> = guard.iter().collect();
        assert_eq!(receipts[0].meta.work_order_id, wo1_id);
        assert_eq!(receipts[1].meta.work_order_id, wo2_id);
    }

    #[tokio::test]
    async fn all_receipts_in_chain_verify() {
        let rt = Runtime::with_default_backends();
        for i in 0..4 {
            run_mock(&rt, &format!("verify {i}")).await;
        }

        let chain = rt.receipt_chain();
        let guard = chain.lock().await;
        for receipt in guard.iter() {
            assert!(verify_hash(receipt));
        }
    }
}

// ===========================================================================
// Module 6: Error propagation through pipeline
// ===========================================================================

mod error_propagation {
    use super::*;

    #[tokio::test]
    async fn unknown_backend_returns_error() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("unknown backend");
        let result = rt.run_streaming("nonexistent", wo).await;
        let err = result.err().expect("expected error");
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[tokio::test]
    async fn failing_backend_returns_backend_failed() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "failing",
            FailingBackend {
                message: "boom".into(),
            },
        );
        let wo = passthrough_wo("fail test");
        let handle = rt.run_streaming("failing", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert!(matches!(
            receipt.unwrap_err(),
            RuntimeError::BackendFailed(_)
        ));
    }

    #[tokio::test]
    async fn partial_fail_backend_still_streams_events() {
        let mut rt = Runtime::new();
        rt.register_backend("partial", PartialFailBackend);
        let wo = passthrough_wo("partial fail");
        let handle = rt.run_streaming("partial", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;

        // Events were sent before failure
        assert!(!events.is_empty());
        // But the receipt is an error
        assert!(receipt.is_err());
    }

    #[tokio::test]
    async fn timeout_on_slow_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("slow", SlowBackend);
        let wo = passthrough_wo("timeout");
        let handle = rt.run_streaming("slow", wo).await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            drain_run(handle).await
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancellation_via_abort() {
        let mut rt = Runtime::new();
        rt.register_backend("slow", SlowBackend);
        let wo = passthrough_wo("cancel");
        let handle = rt.run_streaming("slow", wo).await.unwrap();
        handle.receipt.abort();
        // Should not panic
    }

    #[tokio::test]
    async fn unknown_backend_error_is_not_retryable() {
        let err = RuntimeError::UnknownBackend {
            name: "nope".into(),
        };
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn backend_failed_error_is_retryable() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn capability_check_failure_is_not_retryable() {
        let err = RuntimeError::CapabilityCheckFailed("missing".into());
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn error_code_for_unknown_backend() {
        let err = RuntimeError::UnknownBackend { name: "x".into() };
        assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    }
}

// ===========================================================================
// Module 7: Event multiplexing
// ===========================================================================

mod event_multiplexing {
    use super::*;

    #[tokio::test]
    async fn multiplexer_broadcasts_to_subscribers() {
        let mux = EventMultiplexer::new(64);
        let mut sub1 = mux.subscribe();
        let mut sub2 = mux.subscribe();

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "test".into(),
            },
            ext: None,
        };
        let count = mux.broadcast(ev.clone()).unwrap();
        assert_eq!(count, 2);

        let r1 = sub1.recv().await.unwrap();
        let r2 = sub2.recv().await.unwrap();
        assert!(matches!(r1.kind, AgentEventKind::RunStarted { .. }));
        assert!(matches!(r2.kind, AgentEventKind::RunStarted { .. }));
    }

    #[test]
    fn multiplexer_no_subscribers_returns_error() {
        let mux = EventMultiplexer::new(16);
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "test".into(),
            },
            ext: None,
        };
        assert!(mux.broadcast(ev).is_err());
    }

    #[test]
    fn multiplexer_subscriber_count() {
        let mux = EventMultiplexer::new(16);
        assert_eq!(mux.subscriber_count(), 0);
        let _s1 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 1);
        let _s2 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 2);
    }

    #[test]
    fn event_router_dispatches_by_kind() {
        let mut router = EventRouter::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        router.add_route(
            "run_started",
            Box::new(move |_ev| {
                c.fetch_add(1, Ordering::Relaxed);
            }),
        );

        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "test".into(),
            },
            ext: None,
        };
        router.route(&ev);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_router_ignores_unregistered_kinds() {
        let router = EventRouter::new();
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "test".into(),
            },
            ext: None,
        };
        // Should not panic
        router.route(&ev);
    }

    #[tokio::test]
    async fn multi_event_backend_streams_all_event_types() {
        let mut rt = Runtime::new();
        rt.register_backend("multi", MultiEventBackend { event_count: 3 });
        let wo = passthrough_wo("multi event types");
        let handle = rt.run_streaming("multi", wo).await.unwrap();
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

        assert!(has_started);
        assert!(has_completed);
        assert!(has_message);
        assert!(has_tool_call);
        assert!(has_tool_result);
    }

    #[tokio::test]
    async fn streamed_event_count_matches_trace() {
        let mut rt = Runtime::new();
        rt.register_backend("multi", MultiEventBackend { event_count: 4 });
        let wo = passthrough_wo("count match");
        let handle = rt.run_streaming("multi", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(events.len(), receipt.trace.len());
    }
}

// ===========================================================================
// Module 8: Receipt hashing correctness
// ===========================================================================

mod receipt_hashing {
    use super::*;

    #[tokio::test]
    async fn hash_is_deterministic_across_recomputes() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "determinism").await;

        let h1 = compute_hash(&receipt).unwrap();
        let h2 = compute_hash(&receipt).unwrap();
        let h3 = compute_hash(&receipt).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h2, h3);
    }

    #[tokio::test]
    async fn hash_is_64_char_hex() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "hex check").await;

        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn different_runs_produce_different_hashes() {
        let rt = Runtime::with_default_backends();
        let (_, r1) = run_mock(&rt, "hash run 1").await;
        let (_, r2) = run_mock(&rt, "hash run 2").await;

        // Different run IDs and timestamps should yield different hashes
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[tokio::test]
    async fn tampered_receipt_fails_verification() {
        let rt = Runtime::with_default_backends();
        let (_, mut receipt) = run_mock(&rt, "tamper test").await;

        // Tamper with the outcome
        receipt.outcome = Outcome::Failed;
        assert!(!verify_hash(&receipt));
    }

    #[tokio::test]
    async fn verify_hash_returns_true_for_valid() {
        let rt = Runtime::with_default_backends();
        let (_, receipt) = run_mock(&rt, "verify valid").await;

        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_with_none_hash_passes_verify() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .build();
        // No hash set → verify_hash returns true
        assert!(receipt.receipt_sha256.is_none());
        assert!(verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_with_wrong_hash_fails_verify() {
        let mut receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .build();
        receipt.receipt_sha256 =
            Some("0000000000000000000000000000000000000000000000000000000000000000".into());
        assert!(!verify_hash(&receipt));
    }
}

// ===========================================================================
// Module 9: Runtime construction & backend management
// ===========================================================================

mod runtime_construction {
    use super::*;

    #[test]
    fn new_runtime_has_no_backends() {
        let rt = Runtime::new();
        assert!(rt.backend_names().is_empty());
    }

    #[test]
    fn default_runtime_has_mock() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend_names().contains(&"mock".to_string()));
    }

    #[test]
    fn register_custom_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("custom", MultiEventBackend { event_count: 1 });
        assert!(rt.backend("custom").is_some());
    }

    #[test]
    fn register_multiple_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("a", abp_integrations::MockBackend);
        rt.register_backend("b", MultiEventBackend { event_count: 1 });
        rt.register_backend("c", RichCapabilityBackend);
        assert_eq!(rt.backend_names().len(), 3);
    }

    #[test]
    fn replace_backend_by_name() {
        let mut rt = Runtime::new();
        rt.register_backend("slot", abp_integrations::MockBackend);
        assert_eq!(rt.backend("slot").unwrap().identity().id, "mock");

        rt.register_backend("slot", MultiEventBackend { event_count: 1 });
        assert_eq!(rt.backend("slot").unwrap().identity().id, "multi-event");
    }

    #[test]
    fn backend_lookup_returns_none_for_missing() {
        let rt = Runtime::new();
        assert!(rt.backend("missing").is_none());
    }

    #[test]
    fn registry_contains_check() {
        let rt = Runtime::with_default_backends();
        assert!(rt.registry().contains("mock"));
        assert!(!rt.registry().contains("nonexistent"));
    }

    #[test]
    fn metrics_initially_zero() {
        let rt = Runtime::new();
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 0);
        assert_eq!(snap.successful_runs, 0);
        assert_eq!(snap.failed_runs, 0);
        assert_eq!(snap.total_events, 0);
    }
}

// ===========================================================================
// Module 10: Capability checks
// ===========================================================================

mod capability_checks {
    use super::*;

    #[test]
    fn check_capabilities_passes_for_satisfied() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn check_capabilities_fails_for_unsupported() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        assert!(matches!(
            rt.check_capabilities("mock", &reqs).unwrap_err(),
            RuntimeError::CapabilityCheckFailed(_)
        ));
    }

    #[test]
    fn check_capabilities_empty_requirements_passes() {
        let rt = Runtime::with_default_backends();
        rt.check_capabilities("mock", &CapabilityRequirements::default())
            .unwrap();
    }

    #[test]
    fn check_capabilities_for_unknown_backend() {
        let rt = Runtime::new();
        let reqs = CapabilityRequirements::default();
        assert!(matches!(
            rt.check_capabilities("missing", &reqs).unwrap_err(),
            RuntimeError::UnknownBackend { .. }
        ));
    }

    #[test]
    fn rich_backend_satisfies_multiple_requirements() {
        let mut rt = Runtime::new();
        rt.register_backend("rich", RichCapabilityBackend);
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        rt.check_capabilities("rich", &reqs).unwrap();
    }

    #[tokio::test]
    async fn run_with_capability_requirements_succeeds() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let wo = WorkOrderBuilder::new("cap req test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .requirements(reqs)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn run_with_unsatisfied_capability_fails() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        let wo = WorkOrderBuilder::new("unsatisfied cap")
            .workspace_mode(WorkspaceMode::PassThrough)
            .requirements(reqs)
            .build();
        let result = rt.run_streaming("mock", wo).await;
        assert!(result.is_err());
    }
}

// ===========================================================================
// Module 11: WorkOrder builder variations
// ===========================================================================

mod work_order_variations {
    use super::*;

    #[tokio::test]
    async fn work_order_with_model() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("model test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .model("gpt-4")
            .build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn work_order_with_max_turns() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("turns test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .max_turns(5)
            .build();
        assert_eq!(wo.config.max_turns, Some(5));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn work_order_with_budget() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("budget test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .max_budget_usd(1.50)
            .build();
        assert_eq!(wo.config.max_budget_usd, Some(1.50));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn work_order_with_context_packet() {
        let rt = Runtime::with_default_backends();
        let ctx = ContextPacket {
            files: vec!["src/main.rs".to_string()],
            snippets: vec![ContextSnippet {
                name: "hint".to_string(),
                content: "focus on error handling".to_string(),
            }],
        };
        let wo = WorkOrderBuilder::new("context test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .context(ctx)
            .build();
        assert_eq!(wo.context.files.len(), 1);
        assert_eq!(wo.context.snippets.len(), 1);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn work_order_patch_first_lane() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("patch first")
            .lane(ExecutionLane::PatchFirst)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert_eq!(wo.lane, ExecutionLane::PatchFirst);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn work_order_workspace_first_lane() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("workspace first")
            .lane(ExecutionLane::WorkspaceFirst)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert_eq!(wo.lane, ExecutionLane::WorkspaceFirst);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }
}

// ===========================================================================
// Module 12: Stream pipeline integration
// ===========================================================================

mod stream_pipeline {
    use super::*;

    #[tokio::test]
    async fn runtime_with_stream_pipeline_filter() {
        let pipeline = abp_stream::StreamPipelineBuilder::new()
            .filter(abp_stream::EventFilter::exclude_errors())
            .build();
        let rt = {
            let mut r = Runtime::new().with_stream_pipeline(pipeline);
            r.register_backend("mock", abp_integrations::MockBackend);
            r
        };

        let wo = passthrough_wo("pipeline filter");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;

        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
        // No error events should pass through
        let error_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
            .collect();
        assert!(error_events.is_empty());
    }

    #[tokio::test]
    async fn runtime_with_recorder_pipeline() {
        let recorder = abp_stream::EventRecorder::new();
        let pipeline = abp_stream::StreamPipelineBuilder::new()
            .with_recorder(recorder)
            .build();

        let rt = {
            let mut r = Runtime::new().with_stream_pipeline(pipeline);
            r.register_backend("mock", abp_integrations::MockBackend);
            r
        };

        let wo = passthrough_wo("recorder test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;

        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
        // The pipeline recorder should have captured events
        let pipe = rt.stream_pipeline().unwrap();
        let recorded = pipe.recorder().unwrap();
        assert!(recorded.len() > 0);
        assert_eq!(recorded.len(), events.len());
    }

    #[tokio::test]
    async fn runtime_with_stats_pipeline() {
        let stats = abp_stream::EventStats::new();
        let pipeline = abp_stream::StreamPipelineBuilder::new()
            .with_stats(stats)
            .build();

        let rt = {
            let mut r = Runtime::new().with_stream_pipeline(pipeline);
            r.register_backend("mock", abp_integrations::MockBackend);
            r
        };

        let wo = passthrough_wo("stats test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _ = drain_run(handle).await;

        let pipe = rt.stream_pipeline().unwrap();
        let stats = pipe.stats().unwrap();
        assert!(stats.total_events() > 0);
    }
}
