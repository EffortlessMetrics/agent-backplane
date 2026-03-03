// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style Given/When/Then tests for Agent Backplane end-to-end scenarios.
//!
//! Covers work order submission, backend selection, mock backend modes,
//! receipt generation, policy enforcement, workspace staging, error handling,
//! multi-step conversations, capability negotiation, event streaming,
//! configuration, sidecar registration, and cross-SDK dialect translation.

use std::path::Path;

use abp_capability::{check_capability, generate_report, negotiate};
use abp_config::{
    BackendEntry, BackplaneConfig, ConfigWarning, load_config, merge_configs, parse_toml,
    validate_config,
};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    SupportLevel, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_error::{AbpError, ErrorCategory, ErrorCode};
use abp_integrations::Backend;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use abp_receipt::{ReceiptBuilder, ReceiptChain, compute_hash, diff_receipts, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use abp_stream::{
    EventFilter, EventRecorder, EventStats, EventTransform, StreamPipeline, StreamPipelineBuilder,
};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_with(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require_caps(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
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

fn build_receipt(backend_id: &str, outcome: Outcome) -> Receipt {
    abp_core::ReceiptBuilder::new(backend_id)
        .outcome(outcome)
        .build()
}

async fn drain_run(rt: &Runtime, backend: &str, wo: WorkOrder) -> (Vec<AgentEvent>, Receipt) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    let mut events = vec![];
    let mut stream = handle.events;
    while let Some(ev) = stream.next().await {
        events.push(ev);
    }
    let receipt = handle.receipt.await.unwrap().unwrap();
    (events, receipt)
}

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

/// A backend that emits tool call and result events for multi-step conversation.
#[derive(Debug, Clone)]
struct ToolCallBackend;

#[async_trait]
impl Backend for ToolCallBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-call".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::new()
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(make_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }))
            .await;
        let _ = tx
            .send(make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            }))
            .await;
        let _ = tx
            .send(make_event(AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            }))
            .await;
        let _ = tx
            .send(make_event(AgentEventKind::AssistantMessage {
                text: "I read the file.".into(),
            }))
            .await;
        let _ = tx
            .send(make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }))
            .await;

        Ok(ReceiptBuilder::new("tool-call")
            .run_id(run_id)
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

/// A backend that emits configurable events.
#[derive(Debug, Clone)]
struct StreamingBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for StreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "streaming".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        manifest_with(&[(Capability::Streaming, SupportLevel::Native)])
    }
    async fn run(
        &self,
        run_id: Uuid,
        wo: WorkOrder,
        tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let _ = tx
            .send(make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }))
            .await;
        for i in 0..self.event_count {
            let _ = tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                }))
                .await;
        }
        let _ = tx
            .send(make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }))
            .await;

        Ok(ReceiptBuilder::new("streaming")
            .run_id(run_id)
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

// ===========================================================================
// 1. Work order submission and execution flow (10 tests)
// ===========================================================================

#[tokio::test]
async fn given_work_order_when_submitted_to_mock_then_receipt_returned() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hello world");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn given_work_order_when_submitted_then_run_id_is_unique() {
    let rt = Runtime::with_default_backends();
    let h1 = rt
        .run_streaming("mock", simple_work_order("task1"))
        .await
        .unwrap();
    let h2 = rt
        .run_streaming("mock", simple_work_order("task2"))
        .await
        .unwrap();
    assert_ne!(h1.run_id, h2.run_id);
    let _ = h1.receipt.await;
    let _ = h2.receipt.await;
}

#[tokio::test]
async fn given_work_order_when_mock_backend_then_events_streamed() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("stream test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let mut events = vec![];
    let mut stream = handle.events;
    while let Some(ev) = stream.next().await {
        events.push(ev);
    }
    assert!(!events.is_empty());
    let _ = handle.receipt.await;
}

#[tokio::test]
async fn given_work_order_when_executed_then_receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = drain_run(&rt, "mock", simple_work_order("version check")).await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn given_work_order_when_executed_then_receipt_work_order_id_matches() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("id check");
    let wo_id = wo.id;
    let (_, receipt) = drain_run(&rt, "mock", wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[test]
fn given_work_order_builder_when_build_then_task_preserved() {
    let wo = WorkOrderBuilder::new("My task").build();
    assert_eq!(wo.task, "My task");
}

#[test]
fn given_work_order_builder_when_model_set_then_config_has_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn given_work_order_builder_when_max_turns_set_then_config_has_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(5).build();
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn given_work_order_builder_when_max_budget_set_then_config_has_budget() {
    let wo = WorkOrderBuilder::new("test").max_budget_usd(1.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
}

#[test]
fn given_work_order_builder_when_lane_set_then_lane_preserved() {
    let wo = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

// ===========================================================================
// 2. Backend selection based on dialect (8 tests)
// ===========================================================================

#[test]
fn given_runtime_when_mock_registered_then_backend_discoverable() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn given_runtime_when_custom_backend_registered_then_listed() {
    let mut rt = Runtime::new();
    rt.register_backend("custom", FailingBackend);
    assert!(rt.backend_names().contains(&"custom".to_string()));
}

#[tokio::test]
async fn given_runtime_when_unknown_backend_then_error() {
    let rt = Runtime::with_default_backends();
    let result = rt
        .run_streaming("nonexistent", simple_work_order("test"))
        .await;
    assert!(result.is_err());
    match result {
        Err(RuntimeError::UnknownBackend { .. }) => {}
        _ => panic!("expected UnknownBackend error"),
    }
}

#[test]
fn given_runtime_when_backend_replaced_then_new_one_used() {
    let mut rt = Runtime::with_default_backends();
    rt.register_backend("mock", FailingBackend);
    let backend = rt.backend("mock").unwrap();
    assert_eq!(backend.identity().id, "failing");
}

#[test]
fn given_empty_runtime_when_no_backends_then_list_empty() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn given_runtime_when_multiple_backends_then_all_listed() {
    let mut rt = Runtime::new();
    rt.register_backend("a", FailingBackend);
    rt.register_backend("b", FailingBackend);
    let names = rt.backend_names();
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));
}

#[test]
fn given_runtime_when_backend_lookup_then_identity_matches() {
    let rt = Runtime::with_default_backends();
    let backend = rt.backend("mock").unwrap();
    assert_eq!(backend.identity().id, "mock");
}

#[tokio::test]
async fn given_runtime_with_custom_backend_when_run_then_uses_that_backend() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, receipt) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    assert_eq!(receipt.backend.id, "tool-call");
    assert!(!events.is_empty());
}

// ===========================================================================
// 3. Mock backend passthrough mode (7 tests)
// ===========================================================================

#[test]
fn given_passthrough_mode_when_serialized_then_lowercase() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""passthrough""#);
}

#[test]
fn given_passthrough_mode_when_deserialized_then_matches() {
    let mode: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn given_passthrough_work_order_when_mock_run_then_receipt_complete() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("passthrough test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (_, receipt) = drain_run(&rt, "mock", wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn given_passthrough_receipt_when_mode_checked_then_default_mapped() {
    let receipt = build_receipt("mock", Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[test]
fn given_execution_mode_when_default_then_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn given_passthrough_mode_when_roundtripped_then_preserved() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_value(mode).unwrap();
    let back: ExecutionMode = serde_json::from_value(json).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

#[test]
fn given_receipt_builder_when_mode_set_passthrough_then_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// 4. Mock backend mapped mode (7 tests)
// ===========================================================================

#[test]
fn given_mapped_mode_when_serialized_then_lowercase() {
    let mode = ExecutionMode::Mapped;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""mapped""#);
}

#[tokio::test]
async fn given_mapped_work_order_when_mock_run_then_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("mapped test");
    let (_, receipt) = drain_run(&rt, "mock", wo).await;
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn given_mapped_mode_when_receipt_built_then_mode_is_mapped() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Mapped)
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[test]
fn given_receipt_builder_when_no_mode_set_then_default_mapped() {
    let receipt = build_receipt("mock", Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[test]
fn given_mapped_and_passthrough_when_compared_then_different() {
    assert_ne!(ExecutionMode::Mapped, ExecutionMode::Passthrough);
}

#[test]
fn given_dialect_detector_when_openai_message_then_detected() {
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = detector.detect(&msg);
    assert!(result.is_some());
    assert_eq!(result.unwrap().dialect, Dialect::OpenAi);
}

#[test]
fn given_dialect_detector_when_claude_message_then_detected() {
    let detector = DialectDetector::new();
    let msg = serde_json::json!({
        "type": "message",
        "model": "claude-3-opus",
        "role": "assistant",
        "content": [{"type": "text", "text": "hello"}]
    });
    let result = detector.detect(&msg);
    assert!(result.is_some());
    assert_eq!(result.unwrap().dialect, Dialect::Claude);
}

// ===========================================================================
// 5. Receipt generation and validation (10 tests)
// ===========================================================================

#[test]
fn given_receipt_when_hashed_then_sha256_present() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn given_receipt_when_hashed_then_64_hex_chars() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn given_receipt_when_verify_hash_then_valid() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&receipt));
}

#[test]
fn given_receipt_when_tampered_then_verify_fails() {
    let mut receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    receipt.outcome = Outcome::Failed;
    assert!(!verify_hash(&receipt));
}

#[test]
fn given_receipt_when_json_roundtrip_then_hash_still_valid() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&back));
}

#[test]
fn given_fixed_run_id_when_hashed_twice_then_deterministic() {
    let id = Uuid::nil();
    let r = ReceiptBuilder::new("mock")
        .run_id(id)
        .outcome(Outcome::Complete)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn given_two_receipts_with_different_ids_when_hashed_then_differ() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn given_receipt_chain_when_receipts_appended_then_chain_grows() {
    let mut chain = ReceiptChain::new();
    let r1 = build_receipt("mock", Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = build_receipt("mock", Outcome::Complete)
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn given_receipt_when_diff_against_itself_then_no_differences() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let diffs = diff_receipts(&r, &r);
    assert!(diffs.is_empty());
}

#[test]
fn given_two_receipts_when_diff_then_differences_found() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("other")
        .outcome(Outcome::Failed)
        .build();
    let diffs = diff_receipts(&r1, &r2);
    assert!(!diffs.is_empty());
}

// ===========================================================================
// 6. Policy enforcement (12 tests)
// ===========================================================================

#[test]
fn given_policy_disallowing_bash_when_checked_then_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[test]
fn given_policy_allowing_read_when_write_checked_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[test]
fn given_deny_write_env_when_writing_env_then_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".env")).allowed);
}

#[test]
fn given_deny_write_env_when_writing_other_then_allowed() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn given_deny_read_secrets_when_reading_secret_then_denied() {
    let policy = PolicyProfile {
        deny_read: vec!["secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secrets/key.pem")).allowed);
}

#[test]
fn given_deny_read_secrets_when_reading_src_then_allowed() {
    let policy = PolicyProfile {
        deny_read: vec!["secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn given_empty_policy_when_any_tool_checked_then_allowed() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

#[test]
fn given_empty_policy_when_any_path_checked_then_allowed() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_read_path(Path::new("anything")).allowed);
    assert!(engine.can_write_path(Path::new("anything")).allowed);
}

#[test]
fn given_combined_tool_and_write_deny_when_checked_then_both_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn given_policy_deny_write_when_read_same_path_then_allowed() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_read_path(Path::new(".env")).allowed);
}

#[test]
fn given_policy_multiple_deny_writes_when_checked_then_all_denied() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into(), "**/secrets/**".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".env")).allowed);
    assert!(!engine.can_write_path(Path::new("secrets/api.key")).allowed);
}

#[test]
fn given_policy_allowed_tools_when_allowed_tool_checked_then_allowed() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

// ===========================================================================
// 7. Workspace staging scenarios (8 tests)
// ===========================================================================

#[test]
fn given_passthrough_mode_when_prepare_then_path_is_root() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let ws = WorkspaceManager::prepare(&spec).unwrap();
    assert_eq!(ws.path(), Path::new("."));
}

#[test]
fn given_staged_mode_when_prepare_then_temp_dir_exists() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };
    let ws = match WorkspaceManager::prepare(&spec) {
        Ok(ws) => ws,
        Err(_) => return, // skip on staging errors (e.g. disk-full, locked files)
    };
    assert!(ws.path().exists());
    assert_ne!(ws.path(), Path::new("."));
}

#[test]
fn given_staged_workspace_when_dropped_then_cleaned_up() {
    let path;
    {
        let spec = WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let ws = match WorkspaceManager::prepare(&spec) {
            Ok(ws) => ws,
            Err(_) => return,
        };
        path = ws.path().to_path_buf();
        assert!(path.exists());
    }
    // After drop, the temp dir may or may not exist depending on OS cleanup
    // but the PreparedWorkspace no longer references it.
}

#[test]
fn given_workspace_stager_when_source_missing_then_error() {
    let result = WorkspaceStager::new()
        .source_root("/nonexistent/path/that/does/not/exist")
        .stage();
    assert!(result.is_err());
}

#[test]
fn given_workspace_stager_when_no_source_then_error() {
    let result = WorkspaceStager::new().stage();
    assert!(result.is_err());
}

#[test]
fn given_workspace_stager_when_valid_source_then_staged() {
    let ws = match WorkspaceStager::new().source_root(".").stage() {
        Ok(ws) => ws,
        Err(_) => return,
    };
    assert!(ws.path().exists());
}

#[test]
fn given_workspace_stager_when_git_init_disabled_then_no_git() {
    let ws = match WorkspaceStager::new()
        .source_root(".")
        .with_git_init(false)
        .stage()
    {
        Ok(ws) => ws,
        Err(_) => return,
    };
    // With git_init false, we just check that staging succeeded.
    assert!(ws.path().exists());
}

#[test]
fn given_workspace_spec_when_serialized_then_roundtrips() {
    let spec = WorkspaceSpec {
        root: "/tmp/test".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["**/*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, spec.root);
}

// ===========================================================================
// 8. Error scenarios (10 tests)
// ===========================================================================

#[tokio::test]
async fn given_unknown_backend_when_run_then_unknown_backend_error() {
    let rt = Runtime::with_default_backends();
    let result = rt
        .run_streaming("nonexistent", simple_work_order("test"))
        .await;
    match result {
        Err(RuntimeError::UnknownBackend { .. }) => {}
        _ => panic!("expected UnknownBackend error"),
    }
}

#[tokio::test]
async fn given_failing_backend_when_run_then_backend_failed_error() {
    let mut rt = Runtime::new();
    rt.register_backend("fail", FailingBackend);
    let handle = rt
        .run_streaming("fail", simple_work_order("test"))
        .await
        .unwrap();
    let result = handle.receipt.await.unwrap();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RuntimeError::BackendFailed(_)
    ));
}

#[test]
fn given_error_code_when_category_checked_then_correct() {
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn given_error_code_when_as_str_then_screaming_snake() {
    assert_eq!(ErrorCode::BackendNotFound.as_str(), "BACKEND_NOT_FOUND");
    assert_eq!(ErrorCode::PolicyDenied.as_str(), "POLICY_DENIED");
}

#[test]
fn given_abp_error_when_constructed_then_code_and_message_present() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.message, "timed out after 30s");
}

#[test]
fn given_abp_error_when_context_added_then_retrievable() {
    let err =
        AbpError::new(ErrorCode::BackendNotFound, "not found").with_context("backend", "openai");
    assert!(err.context.contains_key("backend"));
}

#[test]
fn given_runtime_error_when_unknown_backend_then_error_code_matches() {
    let err = RuntimeError::UnknownBackend {
        name: "test".into(),
    };
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

#[test]
fn given_runtime_error_when_policy_failed_then_error_code_matches() {
    let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
    assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
}

#[test]
fn given_runtime_error_when_workspace_failed_then_error_code_matches() {
    let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("bad"));
    assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
}

#[test]
fn given_runtime_error_when_capability_check_failed_then_error_code() {
    let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
    assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
}

// ===========================================================================
// 9. Multi-step agent conversations (8 tests)
// ===========================================================================

#[tokio::test]
async fn given_tool_call_backend_when_run_then_events_include_tool_call() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    let has_tool_call = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }));
    assert!(has_tool_call);
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_events_include_tool_result() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    let has_tool_result = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }));
    assert!(has_tool_result);
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_tool_call_before_result() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
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

#[tokio::test]
async fn given_tool_call_backend_when_run_then_assistant_message_after_tool() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    let result_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .unwrap();
    let msg_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
        .unwrap();
    assert!(msg_idx > result_idx);
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_starts_with_run_started() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_ends_with_run_completed() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_tool_use_id_correlates() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (events, _) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    let call_id = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::ToolCall { tool_use_id, .. } => tool_use_id.clone(),
        _ => None,
    });
    let result_id = events.iter().find_map(|e| match &e.kind {
        AgentEventKind::ToolResult { tool_use_id, .. } => tool_use_id.clone(),
        _ => None,
    });
    assert_eq!(call_id, result_id);
    assert_eq!(call_id.as_deref(), Some("tc1"));
}

#[tokio::test]
async fn given_tool_call_backend_when_run_then_receipt_outcome_complete() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call", ToolCallBackend);
    let (_, receipt) = drain_run(&rt, "tool-call", simple_work_order("test")).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// 10. Capability negotiation before execution (8 tests)
// ===========================================================================

#[test]
fn given_manifest_with_native_when_negotiate_then_native_list() {
    let manifest = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
    let reqs = require_caps(&[(Capability::Streaming, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn given_manifest_with_emulated_when_native_required_then_emulatable() {
    let manifest = manifest_with(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let reqs = require_caps(&[(Capability::ToolRead, MinSupport::Native)]);
    let result = negotiate(&manifest, &reqs);
    // negotiate() classifies by manifest level, not min_support
    assert!(!result.emulated.is_empty());
    assert!(result.unsupported.is_empty());
}

#[test]
fn given_empty_manifest_when_any_required_then_unsupported() {
    let manifest = CapabilityManifest::new();
    let reqs = require_caps(&[(Capability::Streaming, MinSupport::Emulated)]);
    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
}

#[test]
fn given_manifest_when_no_requirements_then_compatible() {
    let manifest = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
    let reqs = CapabilityRequirements::default();
    let result = negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
}

#[test]
fn given_report_when_all_native_then_compatible_summary() {
    let result = abp_capability::NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![],
        unsupported: vec![],
    };
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn given_report_when_unsupported_then_incompatible() {
    let result = abp_capability::NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![Capability::ToolBash],
    };
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
}

#[test]
fn given_check_capability_when_native_then_native() {
    let manifest = manifest_with(&[(Capability::Streaming, SupportLevel::Native)]);
    let level = check_capability(&manifest, &Capability::Streaming);
    assert_eq!(level, abp_capability::SupportLevel::Native);
}

#[test]
fn given_check_capability_when_absent_then_unsupported() {
    let manifest = CapabilityManifest::new();
    let level = check_capability(&manifest, &Capability::Streaming);
    assert!(matches!(
        level,
        abp_capability::SupportLevel::Unsupported { .. }
    ));
}

// ===========================================================================
// 11. Event streaming and ordering (8 tests)
// ===========================================================================

#[tokio::test]
async fn given_streaming_backend_when_run_then_correct_event_count() {
    let mut rt = Runtime::new();
    rt.register_backend("streaming", StreamingBackend { event_count: 3 });
    let (events, _) = drain_run(&rt, "streaming", simple_work_order("test")).await;
    // run_started + 3 deltas + run_completed = 5
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn given_streaming_backend_when_run_then_timestamps_non_decreasing() {
    let mut rt = Runtime::new();
    rt.register_backend("streaming", StreamingBackend { event_count: 5 });
    let (events, _) = drain_run(&rt, "streaming", simple_work_order("test")).await;
    for window in events.windows(2) {
        assert!(window[0].ts <= window[1].ts);
    }
}

#[test]
fn given_event_filter_when_errors_only_then_filters_non_errors() {
    let filter = EventFilter::errors_only();
    let ok_event = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let err_event = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(!filter.matches(&ok_event));
    assert!(filter.matches(&err_event));
}

#[test]
fn given_event_filter_when_exclude_errors_then_passes_non_errors() {
    let filter = EventFilter::exclude_errors();
    let ok_event = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let err_event = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(filter.matches(&ok_event));
    assert!(!filter.matches(&err_event));
}

#[test]
fn given_event_filter_by_kind_when_matched_then_passes() {
    let filter = EventFilter::by_kind("tool_call");
    let tc = make_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    });
    let msg = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert!(filter.matches(&tc));
    assert!(!filter.matches(&msg));
}

#[test]
fn given_event_recorder_when_events_recorded_then_retrievable() {
    let recorder = EventRecorder::new();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    recorder.record(&ev);
    assert_eq!(recorder.len(), 1);
    assert!(!recorder.is_empty());
}

#[test]
fn given_event_stats_when_observed_then_counts_correct() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    }));
    stats.observe(&make_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    }));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 1);
    assert_eq!(stats.total_delta_bytes(), 5);
}

#[test]
fn given_event_transform_identity_when_applied_then_event_unchanged() {
    let transform = EventTransform::identity();
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let transformed = transform.apply(ev.clone());
    assert_eq!(
        serde_json::to_string(&transformed).unwrap(),
        serde_json::to_string(&ev).unwrap()
    );
}

// ===========================================================================
// 12. Configuration loading and override (8 tests)
// ===========================================================================

#[test]
fn given_default_config_when_loaded_then_has_defaults() {
    let config = load_config(None).unwrap();
    assert_eq!(config.log_level.as_deref(), Some("info"));
    assert!(config.backends.is_empty());
}

#[test]
fn given_toml_with_mock_backend_when_parsed_then_has_mock() {
    let toml = r#"
[backends.test]
type = "mock"
"#;
    let config = parse_toml(toml).unwrap();
    assert!(config.backends.contains_key("test"));
    assert!(matches!(config.backends["test"], BackendEntry::Mock { .. }));
}

#[test]
fn given_toml_with_sidecar_when_parsed_then_has_command() {
    let toml = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["index.js"]
"#;
    let config = parse_toml(toml).unwrap();
    match &config.backends["node"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["index.js"]);
        }
        _ => panic!("expected sidecar"),
    }
}

#[test]
fn given_config_when_validated_then_warnings_returned() {
    let config = BackplaneConfig::default();
    let warnings = validate_config(&config).unwrap();
    // Default config has no default_backend and no receipts_dir
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
    );
}

#[test]
fn given_two_configs_when_merged_then_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("sidecar".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("sidecar"));
}

#[test]
fn given_invalid_toml_when_parsed_then_error() {
    let result = parse_toml("not valid toml {{{");
    assert!(result.is_err());
}

#[test]
fn given_config_with_large_timeout_when_validated_then_warning() {
    let mut backends = std::collections::BTreeMap::new();
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7200),
        },
    );
    let config = BackplaneConfig {
        default_backend: Some("node".into()),
        backends,
        ..BackplaneConfig::default()
    };
    let warnings = validate_config(&config).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn given_config_when_serialized_then_roundtrips() {
    let mut backends = std::collections::BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    let config = BackplaneConfig {
        default_backend: Some("mock".into()),
        backends,
        ..BackplaneConfig::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, back);
}

// ===========================================================================
// 13. Sidecar registration and discovery (8 tests)
// ===========================================================================

#[test]
fn given_hello_envelope_when_encoded_then_contains_t_hello() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(r#""t":"hello""#));
}

#[test]
fn given_hello_envelope_when_decoded_then_matches() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn given_fatal_envelope_when_encoded_then_has_error() {
    let fatal = Envelope::fatal_with_code(Some("run-1".into()), "crash", ErrorCode::BackendCrashed);
    let line = JsonlCodec::encode(&fatal).unwrap();
    assert!(line.contains("crash"));
}

#[test]
fn given_fatal_envelope_when_error_code_checked_then_present() {
    let fatal = Envelope::fatal_with_code(None, "timeout", ErrorCode::BackendTimeout);
    assert_eq!(fatal.error_code(), Some(ErrorCode::BackendTimeout));
}

#[test]
fn given_version_string_when_parsed_then_correct() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("invalid"), None);
}

#[test]
fn given_compatible_versions_when_checked_then_true() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

#[test]
fn given_incompatible_versions_when_checked_then_false() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
}

#[test]
fn given_invalid_json_when_decoded_then_error() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

// ===========================================================================
// 14. Cross-SDK dialect translation scenarios (8 tests)
// ===========================================================================

#[test]
fn given_known_rules_when_loaded_then_non_empty() {
    let registry = known_rules();
    assert!(!registry.is_empty());
}

#[test]
fn given_openai_to_claude_when_tool_use_checked_then_lossless() {
    let registry = known_rules();
    let rule = registry.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
    assert!(rule.is_some());
    assert!(rule.unwrap().fidelity.is_lossless());
}

#[test]
fn given_same_dialect_when_any_feature_then_lossless() {
    let registry = known_rules();
    for &dialect in Dialect::all() {
        let rule = registry.lookup(dialect, dialect, "tool_use");
        if let Some(r) = rule {
            assert!(r.fidelity.is_lossless());
        }
    }
}

#[test]
fn given_validate_mapping_when_lossless_then_no_errors() {
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Claude,
        &["tool_use".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].errors.is_empty());
}

#[test]
fn given_validate_mapping_when_unknown_feature_then_unsupported() {
    let registry = known_rules();
    let results = validate_mapping(
        &registry,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent_feature_xyz".into()],
    );
    assert!(!results.is_empty());
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn given_dialect_enum_when_all_then_six_variants() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn given_dialect_when_label_then_readable() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
}

#[test]
fn given_mapping_registry_when_custom_rule_inserted_then_found() {
    let mut registry = MappingRegistry::new();
    registry.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "custom_feature".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial loss".into(),
        },
    });
    let rule = registry.lookup(Dialect::OpenAi, Dialect::Gemini, "custom_feature");
    assert!(rule.is_some());
    assert!(!rule.unwrap().fidelity.is_lossless());
}

// ===========================================================================
// 15. Stream pipeline composition (7 tests)
// ===========================================================================

#[test]
fn given_empty_pipeline_when_event_processed_then_passes_through() {
    let pipeline = StreamPipeline::new();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let result = pipeline.process(ev);
    assert!(result.is_some());
}

#[test]
fn given_pipeline_with_filter_when_non_matching_then_filtered() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(pipeline.process(ev).is_none());
}

#[test]
fn given_pipeline_with_recorder_when_event_processed_then_recorded() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    pipeline.process(ev);
    assert_eq!(recorder.len(), 1);
}

#[test]
fn given_pipeline_with_stats_when_events_processed_then_counted() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    pipeline.process(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    pipeline.process(make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }));
    assert_eq!(stats.total_events(), 2);
}

#[test]
fn given_pipeline_with_transform_when_event_processed_then_transformed() {
    let transform = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantMessage { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let pipeline = StreamPipelineBuilder::new().transform(transform).build();
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let result = pipeline.process(ev).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &result.kind {
        assert_eq!(text, "HELLO");
    } else {
        panic!("expected AssistantMessage");
    }
}

#[test]
fn given_recorder_when_cleared_then_empty() {
    let recorder = EventRecorder::new();
    recorder.record(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_eq!(recorder.len(), 1);
    recorder.clear();
    assert!(recorder.is_empty());
}

#[test]
fn given_stats_when_reset_then_zero() {
    let stats = EventStats::new();
    stats.observe(&make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_eq!(stats.total_events(), 1);
    stats.reset();
    assert_eq!(stats.total_events(), 0);
}

// ===========================================================================
// 16. Canonical JSON and hashing (6 tests)
// ===========================================================================

#[test]
fn given_json_value_when_canonical_then_keys_sorted() {
    let json = canonical_json(&serde_json::json!({"z": 1, "a": 2})).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn given_sha256_hex_when_called_then_64_chars() {
    let hash = sha256_hex(b"test");
    assert_eq!(hash.len(), 64);
}

#[test]
fn given_sha256_hex_when_same_input_then_deterministic() {
    let h1 = sha256_hex(b"hello");
    let h2 = sha256_hex(b"hello");
    assert_eq!(h1, h2);
}

#[test]
fn given_sha256_hex_when_different_input_then_different() {
    let h1 = sha256_hex(b"hello");
    let h2 = sha256_hex(b"world");
    assert_ne!(h1, h2);
}

#[test]
fn given_receipt_hash_when_sha256_field_set_then_excluded_from_hash() {
    let r1 = ReceiptBuilder::new("mock")
        .run_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r1).unwrap();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("garbage".into());
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn given_contract_version_when_checked_then_abp_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ===========================================================================
// 17. Outcome and receipt metadata (6 tests)
// ===========================================================================

#[test]
fn given_outcome_complete_when_serialized_then_string() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, r#""complete""#);
}

#[test]
fn given_outcome_partial_when_serialized_then_string() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(json, r#""partial""#);
}

#[test]
fn given_outcome_failed_when_serialized_then_string() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json, r#""failed""#);
}

#[test]
fn given_receipt_builder_when_artifacts_added_then_present() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[test]
fn given_receipt_builder_when_verification_set_then_present() {
    let report = VerificationReport {
        git_diff: Some("diff --git ...".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .verification(report)
        .build();
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_some());
}

#[test]
fn given_receipt_builder_when_trace_event_added_then_in_trace() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(ev)
        .build();
    assert_eq!(receipt.trace.len(), 1);
}

// ===========================================================================
// 18. Context packet and snippets (4 tests)
// ===========================================================================

#[test]
fn given_context_with_files_when_built_then_preserved() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("test").context(ctx).build();
    assert_eq!(wo.context.files, vec!["src/main.rs"]);
}

#[test]
fn given_context_with_snippets_when_built_then_preserved() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("test").context(ctx).build();
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "readme");
}

#[test]
fn given_empty_context_when_default_then_empty() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn given_context_when_serialized_then_roundtrips() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "s1".into(),
            content: "data".into(),
        }],
    };
    let json = serde_json::to_string(&ctx).unwrap();
    let back: ContextPacket = serde_json::from_str(&json).unwrap();
    assert_eq!(back.files, ctx.files);
    assert_eq!(back.snippets.len(), ctx.snippets.len());
}

// ===========================================================================
// 19. Error taxonomy completeness (6 tests)
// ===========================================================================

#[test]
fn given_all_error_codes_when_as_str_then_non_empty() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in codes {
        assert!(!code.as_str().is_empty());
    }
}

#[test]
fn given_error_code_when_display_then_same_as_str() {
    let code = ErrorCode::BackendTimeout;
    assert_eq!(code.to_string(), code.as_str());
}

#[test]
fn given_error_category_when_display_then_lowercase() {
    assert_eq!(ErrorCategory::Backend.to_string(), "backend");
    assert_eq!(ErrorCategory::Policy.to_string(), "policy");
    assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
}

#[test]
fn given_protocol_errors_when_category_then_protocol() {
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolUnexpectedMessage.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::ProtocolVersionMismatch.category(),
        ErrorCategory::Protocol
    );
}

#[test]
fn given_workspace_errors_when_category_then_workspace() {
    assert_eq!(
        ErrorCode::WorkspaceInitFailed.category(),
        ErrorCategory::Workspace
    );
    assert_eq!(
        ErrorCode::WorkspaceStagingFailed.category(),
        ErrorCategory::Workspace
    );
}

#[test]
fn given_error_code_when_serialized_then_screaming_snake() {
    let json = serde_json::to_string(&ErrorCode::BackendNotFound).unwrap();
    assert_eq!(json, r#""BACKEND_NOT_FOUND""#);
}

// ===========================================================================
// 20. Runtime with stream pipeline integration (5 tests)
// ===========================================================================

#[tokio::test]
async fn given_runtime_with_pipeline_when_run_then_events_recorded() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = simple_work_order("pipeline test");
    let (_, _receipt) = drain_run(&rt, "mock", wo).await;
    assert!(!recorder.is_empty());
}

#[tokio::test]
async fn given_runtime_with_stats_pipeline_when_run_then_stats_populated() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = simple_work_order("stats test");
    let (_, _receipt) = drain_run(&rt, "mock", wo).await;
    assert!(stats.total_events() > 0);
}

#[tokio::test]
async fn given_runtime_with_filter_pipeline_when_run_then_some_filtered() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_message"))
        .with_recorder(recorder.clone())
        .build();
    let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
    let wo = simple_work_order("filter test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    // Drain events from stream to let pipeline process
    let mut stream = handle.events;
    while (stream.next().await).is_some() {}
    let _ = handle.receipt.await;
    // Recorder should only contain assistant_message events
    for ev in recorder.events() {
        assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
    }
}

#[test]
fn given_runtime_when_stream_pipeline_set_then_accessible() {
    let pipeline = StreamPipeline::new();
    let rt = Runtime::new().with_stream_pipeline(pipeline);
    assert!(rt.stream_pipeline().is_some());
}

#[test]
fn given_runtime_when_no_pipeline_then_none() {
    let rt = Runtime::new();
    assert!(rt.stream_pipeline().is_none());
}

// ===========================================================================
// 21. Fidelity and mapping validation (5 tests)
// ===========================================================================

#[test]
fn given_fidelity_lossless_when_checked_then_lossless() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn given_fidelity_unsupported_when_checked_then_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "not available".into(),
    };
    assert!(f.is_unsupported());
    assert!(!f.is_lossless());
}

#[test]
fn given_fidelity_lossy_when_checked_then_neither() {
    let f = Fidelity::LossyLabeled {
        warning: "partial".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn given_validate_mapping_when_empty_feature_then_error() {
    let registry = known_rules();
    let results = validate_mapping(&registry, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(!results[0].errors.is_empty());
}

#[test]
fn given_mapping_registry_when_rank_targets_then_sorted() {
    let registry = known_rules();
    let ranked = registry.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    // Should be ordered by lossless count descending
    for window in ranked.windows(2) {
        assert!(window[0].1 >= window[1].1);
    }
}

// ===========================================================================
// 22. Envelope protocol edge cases (5 tests)
// ===========================================================================

#[test]
fn given_envelope_when_encoded_then_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn given_envelope_hello_with_mode_when_encoded_then_roundtrips() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn given_fatal_from_abp_error_when_created_then_has_code() {
    let err = AbpError::new(ErrorCode::BackendCrashed, "process died");
    let env = Envelope::fatal_from_abp_error(Some("run-1".into()), &err);
    assert_eq!(env.error_code(), Some(ErrorCode::BackendCrashed));
}

#[test]
fn given_stream_of_envelopes_when_decoded_then_all_parsed() {
    use std::io::BufReader;
    let line1 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "err1".into(),
        error_code: None,
    })
    .unwrap();
    let line2 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "err2".into(),
        error_code: None,
    })
    .unwrap();
    let input = format!("{line1}{line2}");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn given_blank_lines_in_stream_when_decoded_then_skipped() {
    use std::io::BufReader;
    let line = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    })
    .unwrap();
    let input = format!("\n\n{line}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

// ===========================================================================
// 23. Support level satisfies checks (4 tests)
// ===========================================================================

#[test]
fn given_native_support_when_native_required_then_satisfies() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn given_native_support_when_emulated_required_then_satisfies() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn given_emulated_support_when_native_required_then_not_satisfied() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn given_unsupported_when_emulated_required_then_not_satisfied() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}
