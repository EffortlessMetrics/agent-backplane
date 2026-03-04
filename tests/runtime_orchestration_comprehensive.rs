#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the ABP runtime orchestration system.
//!
//! Categories:
//! 1. RuntimeBuilder configuration (20+)
//! 2. Backend registration and selection (25+)
//! 3. Work order execution flow (25+)
//! 4. Event streaming (20+)
//! 5. Receipt production (15+)
//! 6. Error handling (15+)

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_root() -> String {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let path = tmp.path().to_string_lossy().to_string();
    std::mem::forget(tmp);
    path
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// A custom backend for testing that always fails.
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
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        _wo: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("intentional failure")
    }
}

/// A custom backend that emits N events before returning.
#[derive(Debug, Clone)]
struct EventCountBackend {
    count: usize,
}

#[async_trait]
impl Backend for EventCountBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "event-count".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        for i in 0..self.count {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            };
            let _ = tx.send(ev).await;
        }
        Ok(ReceiptBuilder::new("event-count")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend with no capabilities (empty manifest) that succeeds.
#[derive(Debug, Clone)]
struct EmptyCapBackend;

#[async_trait]
impl Backend for EmptyCapBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "empty-cap".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        _tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        Ok(ReceiptBuilder::new("empty-cap")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend that sends events slowly.
#[derive(Debug, Clone)]
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
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "slow done".into(),
            },
            ext: None,
        };
        let _ = tx.send(ev).await;
        Ok(ReceiptBuilder::new("slow")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend that emits tool events (ToolCall + ToolResult).
#[derive(Debug, Clone)]
struct ToolBackend;

#[async_trait]
impl Backend for ToolBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-backend".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "tool run starting".into(),
                },
                ext: None,
            })
            .await;
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("t1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
                ext: None,
            })
            .await;
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("t1".into()),
                    output: serde_json::json!({"content": "fn main() {}"}),
                    is_error: false,
                },
                ext: None,
            })
            .await;
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "tool run done".into(),
                },
                ext: None,
            })
            .await;
        Ok(ReceiptBuilder::new("tool-backend")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend that emits error events but still returns a receipt.
#[derive(Debug, Clone)]
struct ErrorEventBackend;

#[async_trait]
impl Backend for ErrorEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-event".into(),
            backend_version: None,
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "something went wrong".into(),
                    error_code: None,
                },
                ext: None,
            })
            .await;
        Ok(ReceiptBuilder::new("error-event")
            .outcome(Outcome::Failed)
            .work_order_id(work_order.id)
            .build())
    }
}

/// A backend that emits many event kinds.
#[derive(Debug, Clone)]
struct AllEventsBackend;

#[async_trait]
impl Backend for AllEventsBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "all-events".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("1.0".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        _run_id: Uuid,
        work_order: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let events = vec![
            AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            AgentEventKind::AssistantDelta {
                text: "hello ".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"cmd": "ls"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!({"stdout": "file.rs"}),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "edited".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            AgentEventKind::Warning {
                message: "low budget".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ];
        for kind in events {
            let _ = tx
                .send(AgentEvent {
                    ts: Utc::now(),
                    kind,
                    ext: None,
                })
                .await;
        }
        Ok(ReceiptBuilder::new("all-events")
            .outcome(Outcome::Complete)
            .work_order_id(work_order.id)
            .build())
    }
}

// ===========================================================================
// 1. RuntimeBuilder configuration (20+ tests)
// ===========================================================================

#[test]
fn config_runtime_new_creates_empty_runtime() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn config_runtime_default_is_same_as_new() {
    let rt = Runtime::default();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn config_runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn config_runtime_with_default_backends_count() {
    let rt = Runtime::with_default_backends();
    assert_eq!(rt.backend_names().len(), 1);
}

#[test]
fn config_runtime_metrics_initially_zero() {
    let rt = Runtime::new();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
}

#[test]
fn config_emulation_config_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn config_projection_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.projection().is_none());
}

#[test]
fn config_stream_pipeline_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn config_registry_returns_reference() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
}

#[test]
fn config_registry_mut_allows_modification() {
    let mut rt = Runtime::new();
    rt.registry_mut().register("test", MockBackend);
    assert!(rt.registry().contains("test"));
}

#[test]
fn config_receipt_chain_is_arc() {
    let rt = Runtime::new();
    let chain1 = rt.receipt_chain();
    let chain2 = rt.receipt_chain();
    assert!(Arc::strong_count(&chain1) >= 2);
    drop(chain2);
}

#[test]
fn config_with_custom_backends_registered() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("failing", FailingBackend);
    rt.register_backend("empty-cap", EmptyCapBackend);
    assert_eq!(rt.backend_names().len(), 3);
}

#[test]
fn config_with_policy_profile_in_work_order() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/node_modules/**".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("test").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["Read"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
}

#[test]
fn config_with_workspace_settings() {
    let wo = WorkOrderBuilder::new("test")
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn config_with_all_options_combined() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("failing", FailingBackend);
    assert_eq!(rt.backend_names().len(), 2);
    assert!(rt.projection().is_none());
    assert!(rt.emulation_config().is_none());
    assert!(rt.stream_pipeline().is_none());
}

#[test]
fn config_runtime_config_model_field() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn config_runtime_config_max_budget() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(25.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(25.0));
}

#[test]
fn config_runtime_config_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(50).build();
    assert_eq!(wo.config.max_turns, Some(50));
}

#[test]
fn config_runtime_config_env_field() {
    let mut config = RuntimeConfig::default();
    config.env.insert("KEY".into(), "VALUE".into());
    let wo = WorkOrderBuilder::new("test").config(config).build();
    assert_eq!(wo.config.env.get("KEY").map(|s| s.as_str()), Some("VALUE"));
}

#[test]
fn config_runtime_config_vendor_field() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    let wo = WorkOrderBuilder::new("test").config(config).build();
    assert!(wo.config.vendor.contains_key("abp"));
}

// ===========================================================================
// 2. Backend registration and selection (25+ tests)
// ===========================================================================

#[test]
fn reg_register_single_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    assert_eq!(rt.backend_names(), vec!["mock".to_string()]);
}

#[test]
fn reg_register_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("failing", FailingBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"failing".to_string()));
    assert!(names.contains(&"mock".to_string()));
    assert_eq!(names.len(), 2);
}

#[test]
fn reg_register_replaces_existing_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("test", MockBackend);
    rt.register_backend("test", FailingBackend);
    assert_eq!(rt.backend_names().len(), 1);
    let b = rt.backend("test").unwrap();
    assert_eq!(b.identity().id, "failing");
}

#[test]
fn reg_backend_lookup_returns_none_for_unknown() {
    let rt = Runtime::new();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn reg_backend_lookup_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[test]
fn reg_backend_names_sorted() {
    let mut rt = Runtime::new();
    rt.register_backend("z-backend", MockBackend);
    rt.register_backend("a-backend", MockBackend);
    rt.register_backend("m-backend", MockBackend);
    let names = rt.backend_names();
    assert_eq!(
        names,
        vec!["a-backend", "m-backend", "z-backend"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn reg_backend_identity_from_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let id = b.identity();
    assert_eq!(id.id, "mock");
    assert!(id.backend_version.is_some());
}

#[test]
fn reg_backend_capabilities_from_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    let caps = b.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn reg_registry_contains_checks() {
    let rt = Runtime::with_default_backends();
    assert!(rt.registry().contains("mock"));
    assert!(!rt.registry().contains("nonexistent"));
}

#[test]
fn reg_registry_list() {
    let rt = Runtime::with_default_backends();
    let list = rt.registry().list();
    assert_eq!(list, vec!["mock"]);
}

#[test]
fn reg_registry_get_arc() {
    let rt = Runtime::with_default_backends();
    let arc = rt.registry().get_arc("mock");
    assert!(arc.is_some());
}

#[test]
fn reg_registry_remove_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("removable", MockBackend);
    assert!(rt.registry().contains("removable"));
    let removed = rt.registry_mut().remove("removable");
    assert!(removed.is_some());
    assert!(!rt.registry().contains("removable"));
}

#[test]
fn reg_registry_remove_nonexistent() {
    let mut rt = Runtime::new();
    let removed = rt.registry_mut().remove("nope");
    assert!(removed.is_none());
}

#[test]
fn reg_registry_default_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn reg_registry_register_and_get() {
    let mut reg = BackendRegistry::default();
    reg.register("mock", MockBackend);
    assert!(reg.get("mock").is_some());
    assert!(reg.get("other").is_none());
}

#[test]
fn reg_registry_register_overwrite() {
    let mut reg = BackendRegistry::default();
    reg.register("test", MockBackend);
    reg.register("test", FailingBackend);
    let b = reg.get("test").unwrap();
    assert_eq!(b.identity().id, "failing");
}

#[test]
fn reg_select_by_name_mock() {
    let rt = Runtime::with_default_backends();
    let b = rt.backend("mock").unwrap();
    assert_eq!(b.identity().id, "mock");
}

#[test]
fn reg_select_by_name_custom() {
    let mut rt = Runtime::new();
    rt.register_backend("custom", EventCountBackend { count: 5 });
    let b = rt.backend("custom").unwrap();
    assert_eq!(b.identity().id, "event-count");
}

#[test]
fn reg_unknown_backend_name_returns_none() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("unknown-xyz").is_none());
}

#[test]
fn reg_capability_check_streaming_native() {
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
fn reg_capability_check_tool_read_emulated() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    rt.check_capabilities("mock", &reqs).unwrap();
}

#[test]
fn reg_capability_check_fails_for_mcp() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    assert!(rt.check_capabilities("mock", &reqs).is_err());
}

#[test]
fn reg_capability_empty_requirements_always_passes() {
    let rt = Runtime::with_default_backends();
    rt.check_capabilities("mock", &CapabilityRequirements::default())
        .unwrap();
}

#[test]
fn reg_five_backends_registered() {
    let mut rt = Runtime::new();
    rt.register_backend("a", MockBackend);
    rt.register_backend("b", FailingBackend);
    rt.register_backend("c", EmptyCapBackend);
    rt.register_backend("d", SlowBackend { delay_ms: 1 });
    rt.register_backend("e", EventCountBackend { count: 3 });
    assert_eq!(rt.backend_names().len(), 5);
    for name in &["a", "b", "c", "d", "e"] {
        assert!(rt.backend(name).is_some());
    }
}

// ===========================================================================
// 3. Work order execution flow (25+ tests)
// ===========================================================================

#[tokio::test]
async fn exec_mock_returns_handle() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("test task");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    assert!(!handle.run_id.is_nil());
}

#[tokio::test]
async fn exec_mock_produces_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("produce receipt");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_mock_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn exec_mock_receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("version test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn exec_mock_receipt_backend_id() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("backend id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn exec_preserves_task_text() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("my specific task");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found = false;
    while let Some(ev) = events.next().await {
        if let AgentEventKind::RunStarted { message } = &ev.kind {
            if message.contains("my specific task") {
                found = true;
            }
        }
    }
    assert!(found, "expected task text in RunStarted message");
}

#[tokio::test]
async fn exec_with_tools_verify_tool_events() {
    let mut rt = Runtime::new();
    rt.register_backend("tool", ToolBackend);
    let wo = simple_work_order("tool test");
    let handle = rt.run_streaming("tool", wo).await.unwrap();
    let mut events = handle.events;
    let mut tool_call_seen = false;
    let mut tool_result_seen = false;
    while let Some(ev) = events.next().await {
        match &ev.kind {
            AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file" => {
                tool_call_seen = true;
            }
            AgentEventKind::ToolResult { tool_name, .. } if tool_name == "read_file" => {
                tool_result_seen = true;
            }
            _ => {}
        }
    }
    assert!(tool_call_seen, "expected ToolCall event");
    assert!(tool_result_seen, "expected ToolResult event");
}

#[tokio::test]
async fn exec_empty_task_succeeds() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_large_context() {
    let rt = Runtime::with_default_backends();
    let big_text = "x".repeat(100_000);
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "big".into(),
            content: big_text,
        }],
    };
    let wo = WorkOrderBuilder::new("large context")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ctx)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_with_staged_workspace() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staged workspace test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_with_passthrough_workspace() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough workspace")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_passthrough_mode_via_vendor_config() {
    let rt = Runtime::with_default_backends();
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    let wo = WorkOrderBuilder::new("passthrough mode test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(config)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_mapped_mode_is_default() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("mapped default");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn exec_with_policy_succeeds() {
    let rt = Runtime::with_default_backends();
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("policy test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_with_context() {
    let rt = Runtime::with_default_backends();
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "test".into(),
            content: "some content".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("context test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ctx)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_with_satisfied_requirements() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("reqs test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_with_unsatisfied_requirements() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("unsatisfied reqs")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(reqs)
        .build();
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exec_with_all_builder_options() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("builder options test")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::PassThrough)
        .lane(ExecutionLane::WorkspaceFirst)
        .model("test-model")
        .max_turns(5)
        .max_budget_usd(10.0)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_long_task_text() {
    let rt = Runtime::with_default_backends();
    let long_task = "a".repeat(10_000);
    let wo = simple_work_order(&long_task);
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_concurrent_runs_different_ids() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("concurrent 1"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("concurrent 2"))
        .await
        .unwrap();
    assert_ne!(h1.run_id, h2.run_id);
    let r1 = h1.receipt.await.unwrap().unwrap();
    let r2 = h2.receipt.await.unwrap().unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn exec_concurrent_runs_both_succeed() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("parallel a"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("parallel b"))
        .await
        .unwrap();
    let (r1, r2) = tokio::join!(h1.receipt, h2.receipt);
    assert_eq!(r1.unwrap().unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_five_concurrent_runs() {
    let rt = Runtime::with_default_backends();
    let mut handles = Vec::new();
    for i in 0..5 {
        let h = rt
            .run_streaming("mock", simple_work_order(&format!("run-{i}")))
            .await
            .unwrap();
        handles.push(h);
    }
    for h in handles {
        let r = h.receipt.await.unwrap().unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn exec_custom_root_workspace() {
    let rt = Runtime::with_default_backends();
    let tmp = tempfile::tempdir().unwrap();
    let wo = WorkOrderBuilder::new("custom root test")
        .root(tmp.path().to_string_lossy().to_string())
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn exec_workspace_git_info_populated() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("git info")
        .root(temp_root())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn exec_empty_cap_backend_skips_cap_check() {
    let mut rt = Runtime::new();
    rt.register_backend("empty-cap", EmptyCapBackend);
    let wo = simple_work_order("empty cap test");
    let handle = rt.run_streaming("empty-cap", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 4. Event streaming (20+ tests)
// ===========================================================================

#[tokio::test]
async fn stream_mock_emits_events() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("event stream");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    assert!(count >= 1, "expected at least one event, got {count}");
}

#[tokio::test]
async fn stream_events_include_run_started() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("run started test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found_start = false;
    while let Some(ev) = events.next().await {
        if matches!(ev.kind, AgentEventKind::RunStarted { .. }) {
            found_start = true;
        }
    }
    assert!(found_start, "expected a RunStarted event");
}

#[tokio::test]
async fn stream_events_include_run_completed() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("run completed test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut found_end = false;
    while let Some(ev) = events.next().await {
        if matches!(ev.kind, AgentEventKind::RunCompleted { .. }) {
            found_end = true;
        }
    }
    assert!(found_end, "expected a RunCompleted event");
}

#[tokio::test]
async fn stream_run_started_is_first_event() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("ordering test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let first = events.next().await.expect("expected at least one event");
    assert!(
        matches!(first.kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got {:?}",
        first.kind
    );
}

#[tokio::test]
async fn stream_run_completed_is_last_event() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("last event test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = handle.events;
    let mut last = None;
    while let Some(ev) = events.next().await {
        last = Some(ev);
    }
    let last = last.expect("expected at least one event");
    assert!(
        matches!(last.kind, AgentEventKind::RunCompleted { .. }),
        "last event should be RunCompleted, got {:?}",
        last.kind
    );
}

#[tokio::test]
async fn stream_all_event_kinds_can_be_streamed() {
    let mut rt = Runtime::new();
    rt.register_backend("all-events", AllEventsBackend);
    let wo = simple_work_order("all events");
    let handle = rt.run_streaming("all-events", wo).await.unwrap();
    let mut events = handle.events;
    let mut kinds = Vec::new();
    while let Some(ev) = events.next().await {
        let kind = match &ev.kind {
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
        };
        kinds.push(kind.to_string());
    }
    assert!(kinds.contains(&"run_started".to_string()));
    assert!(kinds.contains(&"assistant_delta".to_string()));
    assert!(kinds.contains(&"assistant_message".to_string()));
    assert!(kinds.contains(&"tool_call".to_string()));
    assert!(kinds.contains(&"tool_result".to_string()));
    assert!(kinds.contains(&"file_changed".to_string()));
    assert!(kinds.contains(&"command_executed".to_string()));
    assert!(kinds.contains(&"warning".to_string()));
    assert!(kinds.contains(&"run_completed".to_string()));
}

#[tokio::test]
async fn stream_backend_that_sends_no_events() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 0 });
    let wo = simple_work_order("zero events");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while (events.next().await).is_some() {
        count += 1;
    }
    assert_eq!(count, 0, "expected zero events");
}

#[tokio::test]
async fn stream_backend_that_sends_many_events() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 100 });
    let wo = simple_work_order("many events");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    assert_eq!(count, 100);
}

#[tokio::test]
async fn stream_backend_that_sends_error_events() {
    let mut rt = Runtime::new();
    rt.register_backend("error-event", ErrorEventBackend);
    let wo = simple_work_order("error events");
    let handle = rt.run_streaming("error-event", wo).await.unwrap();
    let mut events = handle.events;
    let mut error_seen = false;
    while let Some(ev) = events.next().await {
        if matches!(ev.kind, AgentEventKind::Error { .. }) {
            error_seen = true;
        }
    }
    assert!(error_seen, "expected an Error event");
}

#[tokio::test]
async fn stream_exact_event_count_ten() {
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 10 });
    let wo = simple_work_order("ten events");
    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let mut events = handle.events;
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    assert_eq!(count, 10);
}

#[tokio::test]
async fn stream_receipt_trace_not_empty() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("trace match");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(!receipt.trace.is_empty());
}

#[test]
fn stream_multiplexer_new_has_zero_subscribers() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
}

#[test]
fn stream_multiplexer_subscribe_increments_count() {
    let mux = EventMultiplexer::new(16);
    let _s1 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
    let _s2 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 2);
}

#[test]
fn stream_multiplexer_broadcast_fails_without_subscribers() {
    let mux = EventMultiplexer::new(16);
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(mux.broadcast(ev).is_err());
}

#[tokio::test]
async fn stream_multiplexer_broadcast_reaches_subscribers() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "test".into(),
        },
        ext: None,
    };
    let count = mux.broadcast(ev).unwrap();
    assert_eq!(count, 1);
    let received = sub.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn stream_multiplexer_multi_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub1 = mux.subscribe();
    let mut sub2 = mux.subscribe();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    };
    let count = mux.broadcast(ev).unwrap();
    assert_eq!(count, 2);
    let _ = sub1.recv().await.unwrap();
    let _ = sub2.recv().await.unwrap();
}

#[test]
fn stream_multiplexer_subscriber_drop_decrements() {
    let mux = EventMultiplexer::new(16);
    let sub = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);
    drop(sub);
    assert_eq!(mux.subscriber_count(), 0);
}

#[test]
fn stream_event_router_new_empty() {
    let router = EventRouter::new();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn stream_event_router_routes_matching() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut router = EventRouter::new();
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    router.add_route(
        "assistant_message",
        Box::new(move |_| {
            c.store(true, Ordering::SeqCst);
        }),
    );
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    router.route(&ev);
    assert!(called.load(Ordering::SeqCst));
}

// ===========================================================================
// 5. Receipt production (15+ tests)
// ===========================================================================

#[test]
fn receipt_builder_basic() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_with_hash_produces_sha256() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_is_deterministic() {
    let r = ReceiptBuilder::new("det")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_ignores_existing_hash_field() {
    let mut r = ReceiptBuilder::new("hash-test")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("garbage".to_string());
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "hash should ignore receipt_sha256 field");
}

#[tokio::test]
async fn receipt_completed_run_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash receipt");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_includes_correct_work_order_id() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("wo id");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn receipt_includes_backend_name() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("backend name");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn receipt_success_outcome() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("success outcome");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn receipt_failed_outcome_from_error_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("error-event", ErrorEventBackend);
    let wo = simple_work_order("failed outcome");
    let handle = rt.run_streaming("error-event", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[tokio::test]
async fn receipt_timing_is_populated() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("timing");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn receipt_contract_version_correct() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("version");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_chain_grows_after_run() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", simple_work_order("chain"))
        .await
        .unwrap();
    let _ = handle.receipt.await.unwrap().unwrap();
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert!(!guard.is_empty());
}

#[tokio::test]
async fn receipt_chain_grows_after_two_runs() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("first"))
        .await
        .unwrap();
    let _ = h1.receipt.await.unwrap().unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("second"))
        .await
        .unwrap();
    let _ = h2.receipt.await.unwrap().unwrap();
    let chain = rt.receipt_chain();
    let guard = chain.lock().await;
    assert!(guard.len() >= 2);
}

#[test]
fn receipt_builder_with_hash_convenience() {
    let receipt = ReceiptBuilder::new("conv")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("test").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn receipt_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("caps").capabilities(caps).build();
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_builder_mode_passthrough() {
    let r = ReceiptBuilder::new("mode")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("status".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("ver").verification(v).build();
    assert!(r.verification.harness_ok);
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
}

#[test]
fn receipt_builder_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("usage").usage(usage).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn receipt_builder_usage_raw() {
    let r = ReceiptBuilder::new("test")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 42);
}

#[test]
fn receipt_builder_backend_version() {
    let r = ReceiptBuilder::new("test").backend_version("2.0").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
}

#[test]
fn receipt_builder_adapter_version() {
    let r = ReceiptBuilder::new("test").adapter_version("1.5").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.5"));
}

#[test]
fn receipt_builder_add_trace_event() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "traced".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("test").add_trace_event(ev).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_add_artifact() {
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let r = ReceiptBuilder::new("test").add_artifact(artifact).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

// ===========================================================================
// 6. Error handling (15+ tests)
// ===========================================================================

#[tokio::test]
async fn err_unknown_backend_error() {
    let rt = Runtime::new();
    let wo = simple_work_order("error test");
    let result = rt.run_streaming("nonexistent", wo).await;
    let err = result.err().expect("expected an error");
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

#[tokio::test]
async fn err_unknown_backend_name_in_message() {
    let rt = Runtime::new();
    let wo = simple_work_order("name test");
    let result = rt.run_streaming("my-missing-backend", wo).await;
    let err = result.err().expect("expected an error");
    assert!(err.to_string().contains("my-missing-backend"));
}

#[tokio::test]
async fn err_failing_backend_returns_error() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    let wo = simple_work_order("fail test");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let result = handle.receipt.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn err_failing_backend_error_variant() {
    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    let wo = simple_work_order("fail variant");
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let err = handle.receipt.await.unwrap().unwrap_err();
    assert!(matches!(err, RuntimeError::BackendFailed(_)));
}

#[test]
fn err_unknown_backend_display() {
    let err = RuntimeError::UnknownBackend { name: "foo".into() };
    assert_eq!(err.to_string(), "unknown backend: foo");
}

#[test]
fn err_workspace_failed_display() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.to_string().contains("workspace preparation failed"));
}

#[test]
fn err_policy_failed_display() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("invalid glob"));
    assert!(err.to_string().contains("policy compilation failed"));
}

#[test]
fn err_backend_failed_display() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(err.to_string().contains("backend execution failed"));
}

#[test]
fn err_capability_check_display() {
    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    assert!(err.to_string().contains("missing streaming"));
}

#[test]
fn err_no_projection_match_display() {
    let err = RuntimeError::NoProjectionMatch {
        reason: "no matrix".into(),
    };
    assert!(err.to_string().contains("no matrix"));
}

#[test]
fn err_runtime_error_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<RuntimeError>();
}

#[test]
fn err_runtime_error_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<RuntimeError>();
}

#[test]
fn err_runtime_error_has_display() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let display = format!("{err}");
    assert!(!display.is_empty());
}

#[test]
fn err_runtime_error_has_debug() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());
}

#[test]
fn err_all_variants_have_display() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("ws")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("pol")),
        RuntimeError::BackendFailed(anyhow::anyhow!("be")),
        RuntimeError::CapabilityCheckFailed("cap".into()),
        RuntimeError::NoProjectionMatch {
            reason: "proj".into(),
        },
    ];
    for v in &variants {
        assert!(!v.to_string().is_empty());
    }
}

#[test]
fn err_all_variants_have_debug() {
    let variants: Vec<RuntimeError> = vec![
        RuntimeError::UnknownBackend { name: "x".into() },
        RuntimeError::WorkspaceFailed(anyhow::anyhow!("ws")),
        RuntimeError::PolicyFailed(anyhow::anyhow!("pol")),
        RuntimeError::BackendFailed(anyhow::anyhow!("be")),
        RuntimeError::CapabilityCheckFailed("cap".into()),
        RuntimeError::NoProjectionMatch {
            reason: "proj".into(),
        },
    ];
    for v in &variants {
        assert!(!format!("{v:?}").is_empty());
    }
}

#[test]
fn err_error_code_unknown_backend() {
    let err = RuntimeError::UnknownBackend { name: "x".into() };
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
}

#[test]
fn err_error_code_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
}

#[test]
fn err_error_code_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
}

#[test]
fn err_error_code_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("err"));
    assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
}

#[test]
fn err_into_abp_error() {
    let err = RuntimeError::UnknownBackend {
        name: "gone".into(),
    };
    let abp = err.into_abp_error();
    assert_eq!(abp.code, abp_error::ErrorCode::BackendNotFound);
    assert!(abp.message.contains("gone"));
}

#[test]
fn err_is_retryable_backend_failed() {
    let err = RuntimeError::BackendFailed(anyhow::anyhow!("timeout"));
    assert!(err.is_retryable());
}

#[test]
fn err_is_retryable_workspace_failed() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk"));
    assert!(err.is_retryable());
}

#[test]
fn err_not_retryable_unknown_backend() {
    let err = RuntimeError::UnknownBackend {
        name: "nope".into(),
    };
    assert!(!err.is_retryable());
}

#[test]
fn err_not_retryable_policy_failed() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert!(!err.is_retryable());
}

#[test]
fn err_not_retryable_capability_check() {
    let err = RuntimeError::CapabilityCheckFailed("missing".into());
    assert!(!err.is_retryable());
}

#[test]
fn err_check_capabilities_unknown_backend() {
    let rt = Runtime::new();
    let err = rt
        .check_capabilities("nope", &CapabilityRequirements::default())
        .unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
}

// ===========================================================================
// 7. Telemetry and metrics
// ===========================================================================

#[test]
fn metrics_snapshot_initial() {
    let m = RunMetrics::new();
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
    assert_eq!(snap.average_run_duration_ms, 0);
}

#[test]
fn metrics_record_success() {
    let m = RunMetrics::new();
    m.record_run(100, true, 5);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 5);
}

#[test]
fn metrics_record_failure() {
    let m = RunMetrics::new();
    m.record_run(50, false, 2);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 1);
}

#[test]
fn metrics_multiple_runs() {
    let m = RunMetrics::new();
    m.record_run(100, true, 10);
    m.record_run(200, true, 20);
    m.record_run(300, false, 5);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 35);
    assert_eq!(snap.average_run_duration_ms, 200);
}

#[tokio::test]
async fn metrics_updated_after_run() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("metrics test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _ = handle.receipt.await.unwrap().unwrap();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
}

// ===========================================================================
// 8. Contract types and work order builder
// ===========================================================================

#[test]
fn contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn outcome_serde_complete() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Complete);
}

#[test]
fn outcome_serde_partial() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Partial);
}

#[test]
fn outcome_serde_failed() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    let back: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Outcome::Failed);
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }
}

#[test]
fn support_level_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn canonical_json_sorted_keys() {
    let json = abp_core::canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

#[test]
fn sha256_hex_length() {
    assert_eq!(abp_core::sha256_hex(b"hello").len(), 64);
}

#[test]
fn sha256_hex_deterministic() {
    let h1 = abp_core::sha256_hex(b"test");
    let h2 = abp_core::sha256_hex(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn sha256_hex_different_inputs() {
    assert_ne!(
        abp_core::sha256_hex(b"hello"),
        abp_core::sha256_hex(b"world")
    );
}

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.task, "test");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.config.model.is_none());
}

#[test]
fn work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn work_order_builder_all_options() {
    let wo = WorkOrderBuilder::new("full")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/".into()])
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    assert_eq!(wo.task, "full");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/tmp/test");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

// ===========================================================================
// 9. AgentEvent construction
// ===========================================================================

#[test]
fn agent_event_run_started() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn agent_event_run_completed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn agent_event_assistant_delta() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn agent_event_tool_call() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn agent_event_tool_result() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: None,
            output: serde_json::json!({"content": "data"}),
            is_error: false,
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn agent_event_file_changed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn agent_event_command_executed() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn agent_event_warning() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "low budget".into(),
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn agent_event_error() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "fatal".into(),
            error_code: None,
        },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[test]
fn agent_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"data": 42}));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    assert!(ev.ext.is_some());
    assert!(ev.ext.unwrap().contains_key("raw_message"));
}
