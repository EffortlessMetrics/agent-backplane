// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec)]
//! Cross-SDK end-to-end integration tests.
//!
//! Tests the full ABP pipeline from work order construction through backend
//! execution to receipt verification, using mock backends for each SDK dialect.

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
    WorkspaceMode,
};
use abp_dialect::Dialect;
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_projection::ProjectionMatrix;
use abp_receipt::{ReceiptChain, compute_hash, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Drain all events from a `RunHandle` and return (events, receipt).
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

/// Run on a named backend and drain.
async fn run_full(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    drain_run(handle).await
}

/// Build a pass-through work order.
fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// Build a work order with vendor config for a specific source dialect.
fn dialect_wo(task: &str, dialect_str: &str) -> WorkOrder {
    let abp_config = json!({ "source_dialect": dialect_str });
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), abp_config);
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build()
}

/// Build a work order with passthrough mode and dialect.
fn passthrough_dialect_wo(task: &str, dialect_str: &str) -> WorkOrder {
    let abp_config = json!({ "mode": "passthrough", "source_dialect": dialect_str });
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), abp_config);
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build()
}

/// Build a work order with vendor-specific config for a given dialect provider.
fn vendor_config_wo(
    task: &str,
    dialect_str: &str,
    vendor_key: &str,
    extra: serde_json::Value,
) -> WorkOrder {
    let abp_config = json!({ "source_dialect": dialect_str });
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), abp_config);
    vendor.insert(vendor_key.into(), extra);
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build()
}

/// Standard mock capability manifest.
fn mock_manifest() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolEdit, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    caps
}

/// A custom backend that simulates dialect-specific behavior.
#[derive(Debug, Clone)]
struct DialectMockBackend {
    name: String,
    dialect: Dialect,
    caps: CapabilityManifest,
}

impl DialectMockBackend {
    fn new(name: &str, dialect: Dialect) -> Self {
        Self {
            name: name.into(),
            dialect,
            caps: mock_manifest(),
        }
    }

    fn with_caps(mut self, caps: CapabilityManifest) -> Self {
        self.caps = caps;
        self
    }
}

#[async_trait]
impl Backend for DialectMockBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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
                message: format!("[{}] Processing: {}", self.dialect.label(), work_order.task),
            },
            ext: None,
        };
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        let msg_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!(
                    "Response from {} backend for task: {}",
                    self.name, work_order.task
                ),
            },
            ext: None,
        };
        trace.push(msg_ev.clone());
        let _ = events_tx.send(msg_ev).await;

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("[{}] Done", self.dialect.label()),
            },
            ext: None,
        };
        trace.push(end_ev.clone());
        let _ = events_tx.send(end_ev).await;

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started).num_milliseconds().max(0) as u64;

        let receipt = Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.caps.clone(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({ "dialect": self.dialect.label() }),
            usage: Default::default(),
            trace,
            artifacts: Vec::new(),
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        Ok(receipt.with_hash()?)
    }
}

/// A backend that always fails.
#[derive(Debug, Clone)]
struct FailingBackend {
    error_msg: String,
}

impl FailingBackend {
    fn new(msg: &str) -> Self {
        Self {
            error_msg: msg.into(),
        }
    }
}

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
        mock_manifest()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let err_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: self.error_msg.clone(),
                error_code: None,
            },
            ext: None,
        };
        let _ = events_tx.send(err_ev).await;
        anyhow::bail!("{}", self.error_msg)
    }
}

/// A backend that streams many events.
#[derive(Debug, Clone)]
struct StreamingBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for StreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "streaming".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut caps = BTreeMap::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps
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
                message: format!("Streaming: {}", work_order.task),
            },
            ext: None,
        };
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        for i in 0..self.event_count {
            let delta = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                },
                ext: None,
            };
            trace.push(delta.clone());
            let _ = events_tx.send(delta).await;
        }

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "stream done".into(),
            },
            ext: None,
        };
        trace.push(end_ev.clone());
        let _ = events_tx.send(end_ev).await;

        let finished = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
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
            usage: Default::default(),
            trace,
            artifacts: Vec::new(),
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        Ok(receipt.with_hash()?)
    }
}

/// A backend that simulates tool use.
#[derive(Debug, Clone)]
struct ToolUseBackend;

#[async_trait]
impl Backend for ToolUseBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-use".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        let mut caps = BTreeMap::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        caps.insert(Capability::ToolWrite, SupportLevel::Native);
        caps.insert(Capability::ToolBash, SupportLevel::Native);
        caps
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
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: format!("Tool use: {}", work_order.task),
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "Read".into(),
                    tool_use_id: Some("tc_001".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/main.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "Read".into(),
                    tool_use_id: Some("tc_001".into()),
                    output: json!({"content": "fn main() {}"}),
                    is_error: false,
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "Write".into(),
                    tool_use_id: Some("tc_002".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/lib.rs", "content": "pub fn hello() {}"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "Write".into(),
                    tool_use_id: Some("tc_002".into()),
                    output: json!({"status": "ok"}),
                    is_error: false,
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "I've updated the file.".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "tool use done".into(),
                },
                ext: None,
            },
        ];

        for ev in &events {
            trace.push(ev.clone());
            let _ = events_tx.send(ev.clone()).await;
        }

        let finished = chrono::Utc::now();
        let receipt = Receipt {
            meta: abp_core::RunMetadata {
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
            usage: Default::default(),
            trace,
            artifacts: Vec::new(),
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        Ok(receipt.with_hash()?)
    }
}

// ===========================================================================
// Section 1 – OpenAI mock e2e (7 tests)
// ===========================================================================

#[tokio::test]
async fn openai_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = dialect_wo("Refactor login module", "open_ai");
    let (events, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "openai-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn openai_e2e_receipt_hash_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = dialect_wo("Hash test", "open_ai");
    let (_, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
}

#[tokio::test]
async fn openai_e2e_contract_version() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = dialect_wo("Version test", "open_ai");
    let (_, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn openai_e2e_vendor_config_passthrough() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = vendor_config_wo(
        "Vendor test",
        "open_ai",
        "openai",
        json!({"temperature": 0.7}),
    );
    let (_, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn openai_e2e_event_stream_order() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = dialect_wo("Stream order", "open_ai");
    let (events, _) = run_full(&rt, "openai-mock", wo).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn openai_e2e_model_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = WorkOrderBuilder::new("Model test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gpt-4o")
        .build();
    let (_, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn openai_e2e_timestamps_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-mock",
        DialectMockBackend::new("openai-mock", Dialect::OpenAi),
    );
    let wo = dialect_wo("Timestamp test", "open_ai");
    let (_, receipt) = run_full(&rt, "openai-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

// ===========================================================================
// Section 2 – Claude mock e2e (7 tests)
// ===========================================================================

#[tokio::test]
async fn claude_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = dialect_wo("Analyze codebase", "claude");
    let (events, receipt) = run_full(&rt, "claude-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "claude-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn claude_e2e_receipt_hash_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = dialect_wo("Hash test", "claude");
    let (_, receipt) = run_full(&rt, "claude-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(verify_hash(&receipt));
}

#[tokio::test]
async fn claude_e2e_vendor_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = vendor_config_wo(
        "Claude vendor",
        "claude",
        "anthropic",
        json!({"max_tokens": 8192, "stop_sequences": ["END"]}),
    );
    let (_, receipt) = run_full(&rt, "claude-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn claude_e2e_assistant_message_present() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = dialect_wo("Message test", "claude");
    let (events, _) = run_full(&rt, "claude-mock", wo).await;
    let has_msg = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
    assert!(has_msg, "should have an AssistantMessage event");
}

#[tokio::test]
async fn claude_e2e_run_started_mentions_dialect() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = dialect_wo("Dialect label test", "claude");
    let (events, _) = run_full(&rt, "claude-mock", wo).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains("Claude"),
            "RunStarted should mention Claude dialect"
        );
    } else {
        panic!("first event should be RunStarted");
    }
}

#[tokio::test]
async fn claude_e2e_contract_version() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = dialect_wo("Contract version", "claude");
    let (_, receipt) = run_full(&rt, "claude-mock", wo).await;
    assert_eq!(receipt.unwrap().meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn claude_e2e_model_setting() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-mock",
        DialectMockBackend::new("claude-mock", Dialect::Claude),
    );
    let wo = WorkOrderBuilder::new("Model test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("claude-sonnet-4-20250514")
        .build();
    let (_, receipt) = run_full(&rt, "claude-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// Section 3 – Gemini mock e2e (6 tests)
// ===========================================================================

#[tokio::test]
async fn gemini_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = dialect_wo("Generate content", "gemini");
    let (events, receipt) = run_full(&rt, "gemini-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "gemini-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn gemini_e2e_receipt_hash() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = dialect_wo("Hash check", "gemini");
    let (_, receipt) = run_full(&rt, "gemini-mock", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn gemini_e2e_vendor_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = vendor_config_wo(
        "Gemini vendor",
        "gemini",
        "google",
        json!({"safety_settings": []}),
    );
    let (_, receipt) = run_full(&rt, "gemini-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn gemini_e2e_event_stream() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = dialect_wo("Stream test", "gemini");
    let (events, _) = run_full(&rt, "gemini-mock", wo).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn gemini_e2e_timestamps() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = dialect_wo("Timestamp test", "gemini");
    let (_, receipt) = run_full(&rt, "gemini-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn gemini_e2e_model_setting() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-mock",
        DialectMockBackend::new("gemini-mock", Dialect::Gemini),
    );
    let wo = WorkOrderBuilder::new("Model test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("gemini-pro")
        .build();
    let (_, receipt) = run_full(&rt, "gemini-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// Section 4 – Codex mock e2e (6 tests)
// ===========================================================================

#[tokio::test]
async fn codex_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = dialect_wo("Complete code", "codex");
    let (events, receipt) = run_full(&rt, "codex-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "codex-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn codex_e2e_receipt_hash() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = dialect_wo("Hash check", "codex");
    let (_, receipt) = run_full(&rt, "codex-mock", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn codex_e2e_vendor_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = vendor_config_wo("Codex vendor", "codex", "openai", json!({"suffix": "\n"}));
    let (_, receipt) = run_full(&rt, "codex-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn codex_e2e_event_count() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = dialect_wo("Event count", "codex");
    let (events, receipt) = run_full(&rt, "codex-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn codex_e2e_contract_version() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = dialect_wo("Contract", "codex");
    let (_, receipt) = run_full(&rt, "codex-mock", wo).await;
    assert_eq!(receipt.unwrap().meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn codex_e2e_capabilities() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "codex-mock",
        DialectMockBackend::new("codex-mock", Dialect::Codex),
    );
    let wo = dialect_wo("Caps test", "codex");
    let (_, receipt) = run_full(&rt, "codex-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

// ===========================================================================
// Section 5 – Copilot mock e2e (5 tests)
// ===========================================================================

#[tokio::test]
async fn copilot_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "copilot-mock",
        DialectMockBackend::new("copilot-mock", Dialect::Copilot),
    );
    let wo = dialect_wo("Suggest completion", "copilot");
    let (events, receipt) = run_full(&rt, "copilot-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "copilot-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn copilot_e2e_receipt_hash() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "copilot-mock",
        DialectMockBackend::new("copilot-mock", Dialect::Copilot),
    );
    let wo = dialect_wo("Hash check", "copilot");
    let (_, receipt) = run_full(&rt, "copilot-mock", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn copilot_e2e_event_stream() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "copilot-mock",
        DialectMockBackend::new("copilot-mock", Dialect::Copilot),
    );
    let wo = dialect_wo("Stream test", "copilot");
    let (events, _) = run_full(&rt, "copilot-mock", wo).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn copilot_e2e_timestamps() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "copilot-mock",
        DialectMockBackend::new("copilot-mock", Dialect::Copilot),
    );
    let wo = dialect_wo("Timestamp", "copilot");
    let (_, receipt) = run_full(&rt, "copilot-mock", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn copilot_e2e_vendor_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "copilot-mock",
        DialectMockBackend::new("copilot-mock", Dialect::Copilot),
    );
    let wo = vendor_config_wo(
        "Copilot vendor",
        "copilot",
        "github",
        json!({"editor": "vscode"}),
    );
    let (_, receipt) = run_full(&rt, "copilot-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// Section 6 – Kimi mock e2e (5 tests)
// ===========================================================================

#[tokio::test]
async fn kimi_e2e_basic_run() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "kimi-mock",
        DialectMockBackend::new("kimi-mock", Dialect::Kimi),
    );
    let wo = dialect_wo("Translate text", "kimi");
    let (events, receipt) = run_full(&rt, "kimi-mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "kimi-mock");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn kimi_e2e_receipt_hash() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "kimi-mock",
        DialectMockBackend::new("kimi-mock", Dialect::Kimi),
    );
    let wo = dialect_wo("Hash check", "kimi");
    let (_, receipt) = run_full(&rt, "kimi-mock", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn kimi_e2e_event_stream() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "kimi-mock",
        DialectMockBackend::new("kimi-mock", Dialect::Kimi),
    );
    let wo = dialect_wo("Stream test", "kimi");
    let (events, _) = run_full(&rt, "kimi-mock", wo).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn kimi_e2e_vendor_config() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "kimi-mock",
        DialectMockBackend::new("kimi-mock", Dialect::Kimi),
    );
    let wo = vendor_config_wo(
        "Kimi vendor",
        "kimi",
        "moonshot",
        json!({"use_search": true}),
    );
    let (_, receipt) = run_full(&rt, "kimi-mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn kimi_e2e_contract_version() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "kimi-mock",
        DialectMockBackend::new("kimi-mock", Dialect::Kimi),
    );
    let wo = dialect_wo("Contract", "kimi");
    let (_, receipt) = run_full(&rt, "kimi-mock", wo).await;
    assert_eq!(receipt.unwrap().meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// Section 7 – Cross-dialect e2e (7 tests)
// ===========================================================================

#[tokio::test]
async fn cross_openai_to_claude_backend() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let wo = dialect_wo("Cross-dialect OpenAI to Claude", "open_ai");
    let (events, receipt) = run_full(&rt, "claude-be", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "claude-be");
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn cross_claude_to_openai_backend() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    let wo = dialect_wo("Cross-dialect Claude to OpenAI", "claude");
    let (_, receipt) = run_full(&rt, "openai-be", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "openai-be");
}

#[tokio::test]
async fn cross_openai_to_gemini_backend() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-be",
        DialectMockBackend::new("gemini-be", Dialect::Gemini),
    );
    let wo = dialect_wo("Cross OpenAI to Gemini", "open_ai");
    let (_, receipt) = run_full(&rt, "gemini-be", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn cross_claude_to_gemini_backend() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "gemini-be",
        DialectMockBackend::new("gemini-be", Dialect::Gemini),
    );
    let wo = dialect_wo("Cross Claude to Gemini", "claude");
    let (_, receipt) = run_full(&rt, "gemini-be", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn cross_codex_to_openai_backend() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    let wo = dialect_wo("Cross Codex to OpenAI", "codex");
    let (_, receipt) = run_full(&rt, "openai-be", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn cross_dialect_receipt_hash_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let wo = dialect_wo("Hash cross-dialect", "open_ai");
    let (_, receipt) = run_full(&rt, "claude-be", wo).await;
    let receipt = receipt.unwrap();
    assert!(verify_hash(&receipt));
}

#[tokio::test]
async fn cross_dialect_event_stream_consistent() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let wo = dialect_wo("Events cross-dialect", "open_ai");
    let (events, receipt) = run_full(&rt, "claude-be", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(events.len(), receipt.trace.len());
}

// ===========================================================================
// Section 8 – Passthrough e2e (6 tests)
// ===========================================================================

#[tokio::test]
async fn passthrough_same_dialect() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    let wo = passthrough_dialect_wo("Passthrough same", "open_ai");
    let (_, receipt) = run_full(&rt, "openai-be", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn passthrough_mock_backend() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Simple passthrough");
    let (events, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(events.len() >= 2);
}

#[tokio::test]
async fn passthrough_receipt_hash() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Passthrough hash");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn passthrough_empty_task() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn passthrough_long_task() {
    let rt = Runtime::with_default_backends();
    let long_task = "x".repeat(10_000);
    let wo = passthrough_wo(&long_task);
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn passthrough_contract_version() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Contract check");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// Section 9 – Streaming e2e (7 tests)
// ===========================================================================

#[tokio::test]
async fn streaming_many_deltas() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 50 });
    let wo = passthrough_wo("Stream 50 deltas");
    let (events, receipt) = run_full(&rt, "streaming", wo).await;
    let receipt = receipt.unwrap();
    // RunStarted + 50 deltas + RunCompleted = 52
    assert_eq!(events.len(), 52);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_zero_deltas() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 0 });
    let wo = passthrough_wo("Stream zero");
    let (events, receipt) = run_full(&rt, "streaming", wo).await;
    // RunStarted + RunCompleted = 2
    assert_eq!(events.len(), 2);
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_single_delta() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 1 });
    let wo = passthrough_wo("Stream one");
    let (events, _) = run_full(&rt, "streaming", wo).await;
    assert_eq!(events.len(), 3);
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

#[tokio::test]
async fn streaming_hundred_deltas() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 100 });
    let wo = passthrough_wo("Stream 100");
    let (events, receipt) = run_full(&rt, "streaming", wo).await;
    assert_eq!(events.len(), 102);
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_delta_content_correct() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 5 });
    let wo = passthrough_wo("Content check");
    let (events, _) = run_full(&rt, "streaming", wo).await;
    for (i, ev) in events.iter().enumerate().skip(1).take(5) {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(*text, format!("chunk-{}", i - 1));
        } else {
            panic!("expected AssistantDelta at position {i}");
        }
    }
}

#[tokio::test]
async fn streaming_receipt_hash_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 10 });
    let wo = passthrough_wo("Hash check streaming");
    let (_, receipt) = run_full(&rt, "streaming", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn streaming_events_match_trace() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("streaming", StreamingBackend { event_count: 20 });
    let wo = passthrough_wo("Trace match");
    let (events, receipt) = run_full(&rt, "streaming", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(events.len(), receipt.trace.len());
}

// ===========================================================================
// Section 10 – Multi-backend e2e (6 tests)
// ===========================================================================

#[tokio::test]
async fn multi_backend_route_to_openai() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let wo = passthrough_wo("Route to OpenAI");
    let (_, receipt) = run_full(&rt, "openai-be", wo).await;
    assert_eq!(receipt.unwrap().backend.id, "openai-be");
}

#[tokio::test]
async fn multi_backend_route_to_claude() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let wo = passthrough_wo("Route to Claude");
    let (_, receipt) = run_full(&rt, "claude-be", wo).await;
    assert_eq!(receipt.unwrap().backend.id, "claude-be");
}

#[tokio::test]
async fn multi_backend_route_to_mock() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    let wo = passthrough_wo("Route to mock");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().backend.id, "mock");
}

#[tokio::test]
async fn multi_backend_unknown_errors() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Unknown backend");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn multi_backend_concurrent_runs() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );

    let rt = Arc::new(rt);
    let mut handles = Vec::new();

    for (name, i) in [("mock", 0), ("openai-be", 1), ("claude-be", 2)] {
        let rt_clone = Arc::clone(&rt);
        let task_str = format!("concurrent-{i}");
        let backend_name = name.to_string();
        handles.push(tokio::spawn(async move {
            let wo = passthrough_wo(&task_str);
            let h = rt_clone.run_streaming(&backend_name, wo).await.unwrap();
            let (_, receipt) = drain_run(h).await;
            receipt.unwrap()
        }));
    }

    let mut receipts = Vec::new();
    for h in handles {
        receipts.push(h.await.unwrap());
    }
    assert_eq!(receipts.len(), 3);
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn multi_backend_sequential_different_backends() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );

    let wo1 = passthrough_wo("Step 1");
    let (_, r1) = run_full(&rt, "mock", wo1).await;
    assert_eq!(r1.unwrap().backend.id, "mock");

    let wo2 = passthrough_wo("Step 2");
    let (_, r2) = run_full(&rt, "openai-be", wo2).await;
    assert_eq!(r2.unwrap().backend.id, "openai-be");

    let wo3 = passthrough_wo("Step 3");
    let (_, r3) = run_full(&rt, "claude-be", wo3).await;
    assert_eq!(r3.unwrap().backend.id, "claude-be");
}

// ===========================================================================
// Section 11 – Error e2e (6 tests)
// ===========================================================================

#[tokio::test]
async fn error_backend_failure_propagates() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("failing", FailingBackend::new("simulated crash"));
    let wo = passthrough_wo("Fail test");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Unknown");
    let result = rt.run_streaming("does-not-exist", wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn error_capability_requirement_unsatisfied() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Require native tool read")
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    // MockBackend has ToolRead as Emulated, so requiring Native should fail.
    let result = rt.run_streaming("mock", wo).await;
    // Either returns error directly or the receipt handle fails.
    if let Ok(handle) = result {
        let (_, receipt) = drain_run(handle).await;
        assert!(receipt.is_err());
    }
}

#[tokio::test]
async fn error_failing_backend_sends_error_event() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("failing", FailingBackend::new("event crash"));
    let wo = passthrough_wo("Error event");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let has_error = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    // Either the error event was received before the channel closed,
    // or the receipt itself is an error (backend failure propagated).
    assert!(
        has_error || receipt.is_err(),
        "should have error event or failed receipt"
    );
}

#[tokio::test]
async fn error_no_projection_match() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("No projection");
    let result = rt.select_backend(&wo);
    assert!(result.is_err());
}

#[tokio::test]
async fn error_multiple_failures_independent() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("failing", FailingBackend::new("fail"));

    // First run fails
    let wo1 = passthrough_wo("Fail 1");
    let h1 = rt.run_streaming("failing", wo1).await.unwrap();
    let (_, r1) = drain_run(h1).await;
    assert!(r1.is_err());

    // Second run on mock succeeds
    let wo2 = passthrough_wo("Success after failure");
    let (_, r2) = run_full(&rt, "mock", wo2).await;
    assert_eq!(r2.unwrap().outcome, Outcome::Complete);

    // Third run fails again
    let wo3 = passthrough_wo("Fail 3");
    let h3 = rt.run_streaming("failing", wo3).await.unwrap();
    let (_, r3) = drain_run(h3).await;
    assert!(r3.is_err());
}

// ===========================================================================
// Section 12 – Receipt chain e2e (7 tests)
// ===========================================================================

#[tokio::test]
async fn chain_single_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Chain single");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(receipt).unwrap();
    assert_eq!(chain.len(), 1);
}

#[tokio::test]
async fn chain_multiple_sequential_receipts() {
    let rt = Runtime::with_default_backends();
    let mut chain = ReceiptChain::new();

    for i in 0..5 {
        let wo = passthrough_wo(&format!("Chain step {i}"));
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        chain.push(receipt.unwrap()).unwrap();
    }

    assert_eq!(chain.len(), 5);
}

#[tokio::test]
async fn chain_verify_hashes() {
    let rt = Runtime::with_default_backends();
    let mut chain = ReceiptChain::new();

    for i in 0..3 {
        let wo = passthrough_wo(&format!("Verify step {i}"));
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        chain.push(receipt.unwrap()).unwrap();
    }

    assert!(chain.verify().is_ok());
}

#[tokio::test]
async fn chain_unique_run_ids() {
    let rt = Runtime::with_default_backends();
    let mut ids = Vec::new();

    for i in 0..3 {
        let wo = passthrough_wo(&format!("Unique ID {i}"));
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        let receipt = receipt.unwrap();
        ids.push(receipt.meta.run_id);
    }

    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "all run IDs must be unique");
}

#[tokio::test]
async fn chain_unique_hashes() {
    let rt = Runtime::with_default_backends();
    let mut hashes = Vec::new();

    for i in 0..3 {
        let wo = passthrough_wo(&format!("Unique hash {i}"));
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        let receipt = receipt.unwrap();
        hashes.push(receipt.receipt_sha256.clone().unwrap());
    }

    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique.len(), hashes.len(), "all hashes must be unique");
}

#[tokio::test]
async fn chain_recompute_hash_stable() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("Recompute hash");
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    let receipt = receipt.unwrap();
    let hash1 = compute_hash(&receipt).unwrap();
    let hash2 = compute_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2, "recomputed hashes must be identical");
}

#[tokio::test]
async fn chain_cross_backend_receipts() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );

    let mut chain = ReceiptChain::new();

    let wo1 = passthrough_wo("Chain mock");
    let (_, r1) = run_full(&rt, "mock", wo1).await;
    chain.push(r1.unwrap()).unwrap();

    let wo2 = passthrough_wo("Chain openai");
    let (_, r2) = run_full(&rt, "openai-be", wo2).await;
    chain.push(r2.unwrap()).unwrap();

    let wo3 = passthrough_wo("Chain claude");
    let (_, r3) = run_full(&rt, "claude-be", wo3).await;
    chain.push(r3.unwrap()).unwrap();

    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// Section 13 – Policy + backend e2e (8 tests)
// ===========================================================================

#[tokio::test]
async fn policy_allow_all_tools() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Bash").allowed);
}

#[tokio::test]
async fn policy_deny_tool_before_run() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[tokio::test]
async fn policy_deny_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(std::path::Path::new(".git/config"))
            .allowed
    );
    assert!(
        engine
            .can_write_path(std::path::Path::new("src/main.rs"))
            .allowed
    );
}

#[tokio::test]
async fn policy_deny_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(std::path::Path::new("secrets/api_key.txt"))
            .allowed
    );
    assert!(
        engine
            .can_read_path(std::path::Path::new("src/lib.rs"))
            .allowed
    );
}

#[tokio::test]
async fn policy_allowed_tools_whitelist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[tokio::test]
async fn policy_with_backend_run() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);

    // Run with mock backend still succeeds (policy is advisory pre-check)
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Policy + run test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let (_, receipt) = run_full(&rt, "mock", wo).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn policy_tool_use_backend_with_restrictions() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);

    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // ToolUseBackend would use Read + Write, both allowed
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);

    let wo = WorkOrderBuilder::new("Tool use with policy")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let (events, receipt) = run_full(&rt, "tool-use", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert!(!tool_calls.is_empty());
}

#[tokio::test]
async fn policy_multiple_deny_patterns() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secret*".into(), "**/.env*".into()],
        deny_write: vec!["**/node_modules/**".into(), "**/target/**".into()],
        disallowed_tools: vec!["Bash".into(), "WebSearch".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_read_path(std::path::Path::new("secrets.txt"))
            .allowed
    );
    assert!(
        !engine
            .can_read_path(std::path::Path::new(".env.local"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(std::path::Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(std::path::Path::new("target/debug/main"))
            .allowed
    );
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("WebSearch").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

// ===========================================================================
// Section 14 – Projection e2e (5 tests)
// ===========================================================================

#[tokio::test]
async fn projection_select_backend_by_dialect() {
    let mut pm = ProjectionMatrix::default();
    pm.register_defaults();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("openai-be", mock_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", mock_manifest(), Dialect::Claude, 50);

    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    rt.register_backend(
        "claude-be",
        DialectMockBackend::new("claude-be", Dialect::Claude),
    );
    let rt = rt.with_projection(pm);

    let wo = passthrough_dialect_wo("Project to OpenAI", "open_ai");
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[tokio::test]
async fn projection_select_best_fit() {
    let mut pm = ProjectionMatrix::default();
    pm.register_defaults();
    pm.set_source_dialect(Dialect::Claude);

    let mut high_caps = mock_manifest();
    high_caps.insert(Capability::ExtendedThinking, SupportLevel::Native);

    pm.register_backend("low-prio", mock_manifest(), Dialect::Claude, 10);
    pm.register_backend("high-prio", high_caps, Dialect::Claude, 90);

    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "low-prio",
        DialectMockBackend::new("low-prio", Dialect::Claude),
    );
    rt.register_backend(
        "high-prio",
        DialectMockBackend::new("high-prio", Dialect::Claude).with_caps({
            let mut c = mock_manifest();
            c.insert(Capability::ExtendedThinking, SupportLevel::Native);
            c
        }),
    );
    let rt = rt.with_projection(pm);

    let wo = dialect_wo("Best fit", "claude");
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "high-prio");
}

#[tokio::test]
async fn projection_run_projected() {
    let mut pm = ProjectionMatrix::default();
    pm.register_defaults();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("openai-be", mock_manifest(), Dialect::OpenAi, 50);

    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "openai-be",
        DialectMockBackend::new("openai-be", Dialect::OpenAi),
    );
    let rt = rt.with_projection(pm);

    let wo = passthrough_dialect_wo("Projected run", "open_ai");
    let handle = rt.run_projected(wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn projection_fallback_chain() {
    let mut pm = ProjectionMatrix::default();
    pm.register_defaults();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("primary", mock_manifest(), Dialect::OpenAi, 90);
    pm.register_backend("fallback", mock_manifest(), Dialect::OpenAi, 50);

    let mut rt = Runtime::with_default_backends();
    rt.register_backend(
        "primary",
        DialectMockBackend::new("primary", Dialect::OpenAi),
    );
    rt.register_backend(
        "fallback",
        DialectMockBackend::new("fallback", Dialect::OpenAi),
    );
    let rt = rt.with_projection(pm);

    let wo = dialect_wo("Fallback test", "open_ai");
    let result = rt.select_backend(&wo).unwrap();
    assert_eq!(result.selected_backend, "primary");
    assert!(!result.fallback_chain.is_empty());
}

#[tokio::test]
async fn projection_empty_matrix_errors() {
    let pm = ProjectionMatrix::default();
    let rt = Runtime::with_default_backends().with_projection(pm);
    let wo = passthrough_wo("Empty matrix");
    let result = rt.select_backend(&wo);
    assert!(result.is_err());
}

// ===========================================================================
// Section 15 – Tool use e2e (5 tests)
// ===========================================================================

#[tokio::test]
async fn tool_use_backend_produces_tool_events() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);
    let wo = passthrough_wo("Tool events");
    let (events, receipt) = run_full(&rt, "tool-use", wo).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert_eq!(calls.len(), 2); // Read + Write
}

#[tokio::test]
async fn tool_use_backend_results_present() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);
    let wo = passthrough_wo("Tool results");
    let (events, _) = run_full(&rt, "tool-use", wo).await;
    let results: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(results.len(), 2); // Read result + Write result
}

#[tokio::test]
async fn tool_use_receipt_hash_valid() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);
    let wo = passthrough_wo("Tool hash");
    let (_, receipt) = run_full(&rt, "tool-use", wo).await;
    assert!(verify_hash(&receipt.unwrap()));
}

#[tokio::test]
async fn tool_use_event_order() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);
    let wo = passthrough_wo("Tool order");
    let (events, _) = run_full(&rt, "tool-use", wo).await;
    // RunStarted, ToolCall, ToolResult, ToolCall, ToolResult, AssistantMessage, RunCompleted
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(events[1].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(events[2].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(events[3].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(events[4].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(
        events[5].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        events[6].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn tool_use_capabilities_include_tools() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("tool-use", ToolUseBackend);
    let wo = passthrough_wo("Tool caps");
    let (_, receipt) = run_full(&rt, "tool-use", wo).await;
    let receipt = receipt.unwrap();
    assert!(receipt.capabilities.contains_key(&Capability::ToolRead));
    assert!(receipt.capabilities.contains_key(&Capability::ToolWrite));
    assert!(receipt.capabilities.contains_key(&Capability::ToolBash));
}
