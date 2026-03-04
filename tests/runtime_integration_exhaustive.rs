#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(unknown_lints)]
//! Exhaustive runtime integration tests covering the full pipeline end-to-end.
//!
//! Modules:
//! - `full_pipeline` — WorkOrder → Backend → Events → Receipt
//! - `policy_enforcement` — Tool/path allow/deny integration
//! - `receipt_integrity` — Hashing, trace, metadata, timing
//! - `workspace_staging` — File staging, git init, cleanup
//! - `error_propagation` — Unknown backend, failures, policy errors
//! - `concurrent_execution` — Parallel work orders, event multiplexing, data races

use abp_backend_mock::scenarios::{EventSequenceBuilder, MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RunMetadata, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_policy::PolicyEngine;
use abp_receipt::compute_hash;
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use abp_workspace::{WorkspaceManager, WorkspaceStager};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Shared helpers
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

fn passthrough_wo_with_policy(task: &str, policy: PolicyProfile) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build()
}

async fn run_full(
    rt: &Runtime,
    backend: &str,
    wo: WorkOrder,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    drain_run(handle).await
}

async fn run_mock(rt: &Runtime, task: &str) -> Receipt {
    let wo = passthrough_wo(task);
    let (_, receipt) = run_full(rt, "mock", wo).await;
    receipt.unwrap()
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
    name: String,
    event_count: usize,
}

#[async_trait]
impl Backend for MultiEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.name.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
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

        let start_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: format!("{} starting", self.name),
            },
            ext: None,
        };
        trace.push(start_ev.clone());
        let _ = events_tx.send(start_ev).await;

        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let end_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: format!("{} done", self.name),
            },
            ext: None,
        };
        trace.push(end_ev.clone());
        let _ = events_tx.send(end_ev).await;

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({"note": "multi-event"}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// A backend that emits tool call and tool result events.
#[derive(Debug, Clone)]
struct ToolCallBackend;

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
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m.insert(Capability::ToolRead, SupportLevel::Native);
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

        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "tool-call starting".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            AgentEventKind::AssistantMessage {
                text: "Read the file successfully".into(),
            },
            AgentEventKind::RunCompleted {
                message: "tool-call done".into(),
            },
        ];

        for kind in kinds {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({"note": "tool-call"}),
            usage: UsageNormalized {
                input_tokens: Some(50),
                output_tokens: Some(25),
                estimated_cost_usd: Some(0.001),
                ..Default::default()
            },
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

/// A backend that emits an error event but still completes.
#[derive(Debug, Clone)]
struct ErrorEventBackend;

#[async_trait]
impl Backend for ErrorEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "error-event".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
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

        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "error-event starting".into(),
            },
            AgentEventKind::Error {
                message: "something went wrong".into(),
                error_code: None,
            },
            AgentEventKind::Warning {
                message: "partial failure".into(),
            },
            AgentEventKind::RunCompleted {
                message: "error-event done".into(),
            },
        ];

        for kind in kinds {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind,
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Partial,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ===========================================================================
// Module: full_pipeline — WorkOrder → Backend → Events → Receipt
// ===========================================================================

mod full_pipeline {
    use super::*;

    #[tokio::test]
    async fn mock_backend_happy_path_returns_complete_receipt() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "happy path test").await;
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.backend.id, "mock");
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn mock_backend_produces_events_before_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("events before receipt");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        assert!(!events.is_empty());
        assert!(receipt.is_ok());
    }

    #[tokio::test]
    async fn mock_backend_first_event_is_run_started() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_full(&rt, "mock", passthrough_wo("first event")).await;
        assert!(matches!(
            events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ));
    }

    #[tokio::test]
    async fn mock_backend_last_event_is_run_completed() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_full(&rt, "mock", passthrough_wo("last event")).await;
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn mock_backend_events_include_assistant_messages() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_full(&rt, "mock", passthrough_wo("assistant msgs")).await;
        let has_msg = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
        assert!(has_msg, "expected at least one AssistantMessage event");
    }

    #[tokio::test]
    async fn tool_call_backend_emits_tool_events() {
        let mut rt = Runtime::new();
        rt.register_backend("tool", ToolCallBackend);
        let (events, receipt) = run_full(&rt, "tool", passthrough_wo("tool test")).await;
        let receipt = receipt.unwrap();

        let has_call = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }));
        let has_result = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }));

        assert!(has_call, "expected ToolCall event");
        assert!(has_result, "expected ToolResult event");
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn tool_call_backend_trace_contains_tool_events() {
        let mut rt = Runtime::new();
        rt.register_backend("tool", ToolCallBackend);
        let (_, receipt) = run_full(&rt, "tool", passthrough_wo("tool trace")).await;
        let receipt = receipt.unwrap();

        let tool_calls: Vec<_> = receipt
            .trace
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
            .collect();
        assert!(!tool_calls.is_empty());
    }

    #[tokio::test]
    async fn error_event_backend_returns_partial_outcome() {
        let mut rt = Runtime::new();
        rt.register_backend("err-ev", ErrorEventBackend);
        let (events, receipt) = run_full(&rt, "err-ev", passthrough_wo("error events")).await;
        let receipt = receipt.unwrap();

        let has_error = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Error { .. }));
        let has_warning = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Warning { .. }));

        assert!(has_error);
        assert!(has_warning);
        assert_eq!(receipt.outcome, Outcome::Partial);
    }

    #[tokio::test]
    async fn multi_event_backend_streams_correct_count() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "multi",
            MultiEventBackend {
                name: "multi".into(),
                event_count: 10,
            },
        );
        let (events, receipt) = run_full(&rt, "multi", passthrough_wo("count test")).await;
        assert!(receipt.is_ok());
        // RunStarted + 10 deltas + RunCompleted = 12
        assert_eq!(events.len(), 12);
    }

    #[tokio::test]
    async fn scenario_streaming_success_emits_chunks() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "stream",
            ScenarioMockBackend::new(MockScenario::StreamingSuccess {
                chunks: vec!["a".into(), "b".into(), "c".into()],
                chunk_delay_ms: 0,
            }),
        );
        let (events, receipt) = run_full(&rt, "stream", passthrough_wo("streaming")).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);

        let deltas: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
            .collect();
        assert_eq!(deltas.len(), 3);
    }

    #[tokio::test]
    async fn scenario_custom_with_file_changed_events() {
        let scenario = EventSequenceBuilder::new()
            .message("analyzing")
            .file_changed("src/main.rs", "added function")
            .file_changed("src/lib.rs", "updated module")
            .message("done editing")
            .build();

        let mut rt = Runtime::new();
        rt.register_backend("custom", ScenarioMockBackend::new(scenario));
        let (events, receipt) = run_full(&rt, "custom", passthrough_wo("file changes")).await;
        let receipt = receipt.unwrap();

        let file_changes: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::FileChanged { .. }))
            .collect();
        assert_eq!(file_changes.len(), 2);
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn scenario_custom_with_command_executed() {
        let scenario = EventSequenceBuilder::new()
            .command_executed("cargo test", 0, Some("all passed".into()))
            .build();

        let mut rt = Runtime::new();
        rt.register_backend("cmd", ScenarioMockBackend::new(scenario));
        let (events, _) = run_full(&rt, "cmd", passthrough_wo("command exec")).await;

        let cmds: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::CommandExecuted { .. }))
            .collect();
        assert_eq!(cmds.len(), 1);
    }

    #[tokio::test]
    async fn run_handle_provides_unique_run_id() {
        let rt = Runtime::with_default_backends();
        let h1 = rt
            .run_streaming("mock", passthrough_wo("id1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", passthrough_wo("id2"))
            .await
            .unwrap();
        assert_ne!(h1.run_id, h2.run_id);
        let _ = drain_run(h1).await;
        let _ = drain_run(h2).await;
    }

    #[tokio::test]
    async fn receipt_work_order_id_matches_input() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("wo id match");
        let expected_id = wo.id;
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        assert_eq!(receipt.unwrap().meta.work_order_id, expected_id);
    }
}

// ===========================================================================
// Module: policy_enforcement — Tool/path allow/deny integration
// ===========================================================================

mod policy_enforcement {
    use super::*;

    #[test]
    fn allowed_tool_passes_policy_check() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Write").allowed);
    }

    #[test]
    fn denied_tool_blocked_by_policy() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_use_tool("Bash").allowed);
    }

    #[test]
    fn deny_overrides_allow_for_same_tool() {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".into()],
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
    }

    #[test]
    fn empty_allow_list_permits_all_tools() {
        let policy = PolicyProfile::default();
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("anything").allowed);
    }

    #[test]
    fn non_empty_allow_list_denies_unlisted_tools() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(!engine.can_use_tool("Write").allowed);
    }

    #[test]
    fn deny_read_path_enforcement() {
        let policy = PolicyProfile {
            deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(
            !engine
                .can_read_path(Path::new("secrets/api_key.txt"))
                .allowed
        );
        assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_path_enforcement() {
        let policy = PolicyProfile {
            deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
        assert!(
            !engine
                .can_write_path(Path::new("node_modules/pkg/index.js"))
                .allowed
        );
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn combined_read_write_policy() {
        let policy = PolicyProfile {
            deny_read: vec!["**/secret*".into()],
            deny_write: vec!["**/locked/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
        assert!(engine.can_write_path(Path::new("secret.txt")).allowed);
        assert!(engine.can_read_path(Path::new("locked/data.txt")).allowed);
        assert!(!engine.can_write_path(Path::new("locked/data.txt")).allowed);
    }

    #[test]
    fn glob_wildcard_patterns_for_tools() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash*".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_use_tool("BashExec").allowed);
        assert!(!engine.can_use_tool("BashRun").allowed);
        assert!(engine.can_use_tool("Read").allowed);
    }

    #[tokio::test]
    async fn runtime_compiles_policy_for_work_order() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        };
        let wo = passthrough_wo_with_policy("policy test", policy);
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn runtime_rejects_invalid_policy_globs() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            disallowed_tools: vec!["[invalid".into()],
            ..PolicyProfile::default()
        };
        let wo = passthrough_wo_with_policy("bad policy", policy);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert!(receipt.is_err());
        let err = receipt.unwrap_err();
        assert!(
            matches!(err, RuntimeError::PolicyFailed(_)),
            "expected PolicyFailed, got {err:?}"
        );
    }

    #[test]
    fn policy_decision_allow_has_no_reason() {
        let d = abp_policy::Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn policy_decision_deny_has_reason() {
        let d = abp_policy::Decision::deny("not allowed");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("not allowed"));
    }

    #[test]
    fn deep_nested_path_deny() {
        let policy = PolicyProfile {
            deny_write: vec!["secret/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
        assert!(engine.can_write_path(Path::new("public/data.txt")).allowed);
    }
}

// ===========================================================================
// Module: receipt_integrity — Hashing, trace, metadata, timing
// ===========================================================================

mod receipt_integrity {
    use super::*;

    #[tokio::test]
    async fn receipt_hash_is_present() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "hash present").await;
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn receipt_hash_is_hex_64_chars() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "hash format").await;
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex"
        );
    }

    #[tokio::test]
    async fn receipt_hash_is_deterministic() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "deterministic hash").await;
        let original_hash = receipt.receipt_sha256.clone().unwrap();
        let recomputed = compute_hash(&receipt).unwrap();
        assert_eq!(original_hash, recomputed);
    }

    #[tokio::test]
    async fn receipt_hash_changes_with_different_tasks() {
        let rt = Runtime::with_default_backends();
        let r1 = run_mock(&rt, "task alpha").await;
        let r2 = run_mock(&rt, "task beta").await;
        // Different tasks should produce different hashes (different trace content)
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[tokio::test]
    async fn receipt_trace_is_non_empty() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "trace check").await;
        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn receipt_trace_contains_run_started() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "trace started").await;
        let has_started = receipt
            .trace
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }));
        assert!(has_started);
    }

    #[tokio::test]
    async fn receipt_trace_contains_run_completed() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "trace completed").await;
        let has_completed = receipt
            .trace
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
        assert!(has_completed);
    }

    #[tokio::test]
    async fn receipt_contract_version_matches() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "version check").await;
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn receipt_backend_identity_matches_mock() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "identity check").await;
        assert_eq!(receipt.backend.id, "mock");
        assert!(receipt.backend.backend_version.is_some());
    }

    #[tokio::test]
    async fn receipt_timing_is_reasonable() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "timing check").await;
        assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    }

    #[tokio::test]
    async fn receipt_duration_is_consistent() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "duration check").await;
        let computed_ms = (receipt.meta.finished_at - receipt.meta.started_at)
            .num_milliseconds()
            .unsigned_abs();
        // Allow some tolerance (runtime recomputes hash etc.)
        assert!(
            receipt.meta.duration_ms <= computed_ms + 100,
            "duration_ms {} should be close to computed {}",
            receipt.meta.duration_ms,
            computed_ms
        );
    }

    #[tokio::test]
    async fn receipt_events_in_chronological_order() {
        let rt = Runtime::with_default_backends();
        let (events, _) = run_full(&rt, "mock", passthrough_wo("chrono order")).await;
        for window in events.windows(2) {
            assert!(
                window[0].ts <= window[1].ts,
                "events out of order: {:?} > {:?}",
                window[0].ts,
                window[1].ts
            );
        }
    }

    #[tokio::test]
    async fn receipt_usage_fields_present_for_mock() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "usage fields").await;
        assert!(receipt.usage.input_tokens.is_some());
        assert!(receipt.usage.output_tokens.is_some());
    }

    #[tokio::test]
    async fn receipt_verification_report_harness_ok() {
        let rt = Runtime::with_default_backends();
        let receipt = run_mock(&rt, "harness check").await;
        assert!(receipt.verification.harness_ok);
    }

    #[tokio::test]
    async fn multi_event_receipt_trace_matches_event_count() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "multi5",
            MultiEventBackend {
                name: "multi5".into(),
                event_count: 5,
            },
        );
        let (_, receipt) = run_full(&rt, "multi5", passthrough_wo("trace count")).await;
        let receipt = receipt.unwrap();
        // RunStarted + 5 deltas + RunCompleted = 7
        assert_eq!(receipt.trace.len(), 7);
    }

    #[tokio::test]
    async fn scenario_custom_usage_tokens_in_receipt() {
        let scenario = EventSequenceBuilder::new()
            .message("hello")
            .usage_tokens(100, 50)
            .build();

        let mut rt = Runtime::new();
        rt.register_backend("tokens", ScenarioMockBackend::new(scenario));
        let (_, receipt) = run_full(&rt, "tokens", passthrough_wo("token check")).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.usage.input_tokens, Some(100));
        assert_eq!(receipt.usage.output_tokens, Some(50));
    }
}

// ===========================================================================
// Module: workspace_staging — File staging, git init, cleanup
// ===========================================================================

mod workspace_staging {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn passthrough_workspace_uses_original_path() {
        let tmp = tempdir().unwrap();
        let spec = abp_core::WorkspaceSpec {
            root: tmp.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        assert_eq!(prepared.path(), tmp.path());
        assert!(!prepared.is_staged());
    }

    #[test]
    fn staged_workspace_creates_temp_directory() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("hello.txt"), "world").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        assert!(prepared.is_staged());
        assert_ne!(prepared.path(), source.path());
    }

    #[test]
    fn staged_workspace_copies_files() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("file.txt"), "content").unwrap();
        std::fs::create_dir_all(source.path().join("sub")).unwrap();
        std::fs::write(source.path().join("sub/nested.txt"), "nested").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        assert!(prepared.path().join("file.txt").exists());
        assert!(prepared.path().join("sub/nested.txt").exists());
    }

    #[test]
    fn staged_workspace_has_git_initialized() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("test.txt"), "data").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        assert!(prepared.path().join(".git").is_dir());
    }

    #[test]
    fn staged_workspace_validates_successfully() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("test.txt"), "data").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        let validation = prepared.validate();
        assert!(validation.is_valid());
    }

    #[test]
    fn staged_workspace_excludes_git_directory() {
        let source = tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".git/objects")).unwrap();
        std::fs::write(source.path().join(".git/config"), "gitconfig").unwrap();
        std::fs::write(source.path().join("src.txt"), "source").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        assert!(prepared.path().join("src.txt").exists());
        // .git should be a fresh git init, not the copied source .git
        assert!(prepared.path().join(".git").exists());
    }

    #[test]
    fn staged_workspace_cleanup_removes_temp_dir() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("test.txt"), "data").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        let staged_path = prepared.path().to_path_buf();
        assert!(staged_path.exists());
        prepared.cleanup().unwrap();
        assert!(!staged_path.exists());
    }

    #[test]
    fn workspace_metadata_reports_correct_file_count() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("a.txt"), "a").unwrap();
        std::fs::write(source.path().join("b.txt"), "b").unwrap();
        std::fs::write(source.path().join("c.txt"), "c").unwrap();

        let spec = abp_core::WorkspaceSpec {
            root: source.path().to_string_lossy().to_string(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        };
        let prepared = WorkspaceManager::prepare(&spec).unwrap();
        let meta = prepared.metadata().unwrap();
        assert_eq!(meta.file_count, 3);
    }

    #[tokio::test]
    async fn staged_workspace_through_runtime_succeeds() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("test.txt"), "hello").unwrap();

        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("staged ws test")
            .root(source.path().to_string_lossy().to_string())
            .workspace_mode(WorkspaceMode::Staged)
            .build();
        let (_, receipt) = run_full(&rt, "mock", wo).await;
        assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
    }

    #[test]
    fn workspace_stager_builder_api() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("include_me.txt"), "yes").unwrap();
        std::fs::write(source.path().join("skip.log"), "no").unwrap();

        let ws = WorkspaceStager::new()
            .source_root(source.path())
            .exclude(vec!["*.log".into()])
            .stage()
            .unwrap();

        assert!(ws.path().join("include_me.txt").exists());
        assert!(!ws.path().join("skip.log").exists());
    }

    #[test]
    fn workspace_stager_without_git_init() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("f.txt"), "data").unwrap();

        let ws = WorkspaceStager::new()
            .source_root(source.path())
            .with_git_init(false)
            .stage()
            .unwrap();

        assert!(!ws.path().join(".git").exists());
    }

    #[test]
    fn workspace_content_hash_is_deterministic() {
        let source = tempdir().unwrap();
        std::fs::write(source.path().join("a.txt"), "hello").unwrap();
        std::fs::write(source.path().join("b.txt"), "world").unwrap();

        let h1 = abp_workspace::workspace_content_hash(source.path()).unwrap();
        let h2 = abp_workspace::workspace_content_hash(source.path()).unwrap();
        assert_eq!(h1, h2);
    }
}

// ===========================================================================
// Module: error_propagation — Unknown backend, failures, policy errors
// ===========================================================================

mod error_propagation {
    use super::*;

    #[tokio::test]
    async fn unknown_backend_returns_error() {
        let rt = Runtime::with_default_backends();
        let err = match rt
            .run_streaming("nonexistent", passthrough_wo("unknown"))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("expected error for unknown backend"),
        };
        assert!(
            matches!(err, RuntimeError::UnknownBackend { ref name } if name == "nonexistent"),
            "expected UnknownBackend, got {err:?}"
        );
    }

    #[tokio::test]
    async fn unknown_backend_error_code() {
        let rt = Runtime::with_default_backends();
        let err = match rt.run_streaming("nope", passthrough_wo("err code")).await {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    }

    #[tokio::test]
    async fn unknown_backend_not_retryable() {
        let rt = Runtime::with_default_backends();
        let err = match rt
            .run_streaming("missing", passthrough_wo("retryable"))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(!err.is_retryable());
    }

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
            .run_streaming("fail", passthrough_wo("fail test"))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert!(
            matches!(err, RuntimeError::BackendFailed(_)),
            "expected BackendFailed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn backend_failed_is_retryable() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "transient".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", passthrough_wo("retryable fail"))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn backend_failed_error_code() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "crash".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", passthrough_wo("err code fail"))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
    }

    #[tokio::test]
    async fn policy_failure_from_invalid_glob() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            deny_read: vec!["[bad-glob".into()],
            ..PolicyProfile::default()
        };
        let wo = passthrough_wo_with_policy("bad glob", policy);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert!(matches!(
            receipt.unwrap_err(),
            RuntimeError::PolicyFailed(_)
        ));
    }

    #[tokio::test]
    async fn policy_failure_not_retryable() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            allowed_tools: vec!["[invalid".into()],
            ..PolicyProfile::default()
        };
        let wo = passthrough_wo_with_policy("non-retryable", policy);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn policy_failure_error_code() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            deny_write: vec!["[bad".into()],
            ..PolicyProfile::default()
        };
        let wo = passthrough_wo_with_policy("pol err code", policy);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
    }

    #[tokio::test]
    async fn scenario_permanent_error_propagates() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "perm",
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "ERR-001".into(),
                message: "permanent".into(),
            }),
        );
        let handle = rt
            .run_streaming("perm", passthrough_wo("permanent error"))
            .await
            .unwrap();
        let (_, receipt) = drain_run(handle).await;
        assert!(receipt.is_err());
    }

    #[tokio::test]
    async fn scenario_fail_after_events_propagates_error() {
        let scenario = EventSequenceBuilder::new()
            .message("before crash")
            .fail_after("mid-stream crash")
            .build();

        let mut rt = Runtime::new();
        rt.register_backend("crash", ScenarioMockBackend::new(scenario));
        let handle = rt
            .run_streaming("crash", passthrough_wo("crash test"))
            .await
            .unwrap();
        let (_events, receipt) = drain_run(handle).await;
        // The backend crashes after emitting events; runtime surfaces BackendFailed
        assert!(receipt.is_err());
        assert!(matches!(
            receipt.unwrap_err(),
            RuntimeError::BackendFailed(_)
        ));
    }

    #[tokio::test]
    async fn capability_check_failure_for_missing_capability() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        let err = rt.check_capabilities("mock", &reqs).unwrap_err();
        assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
    }

    #[tokio::test]
    async fn capability_check_for_unknown_backend() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements::default();
        let err = rt.check_capabilities("ghost", &reqs).unwrap_err();
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[tokio::test]
    async fn runtime_error_display_messages() {
        let e1 = RuntimeError::UnknownBackend { name: "foo".into() };
        assert!(e1.to_string().contains("foo"));

        let e2 = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
        assert!(e2.to_string().contains("backend execution failed"));

        let e3 = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
        assert!(e3.to_string().contains("policy"));
    }
}

// ===========================================================================
// Module: concurrent_execution — Parallel work orders, multiplexing, races
// ===========================================================================

mod concurrent_execution {
    use super::*;

    #[tokio::test]
    async fn two_simultaneous_mock_runs() {
        let rt = Runtime::with_default_backends();
        let h1 = rt
            .run_streaming("mock", passthrough_wo("concurrent-1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", passthrough_wo("concurrent-2"))
            .await
            .unwrap();

        let (e1, r1) = drain_run(h1).await;
        let (e2, r2) = drain_run(h2).await;

        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert!(!e1.is_empty());
        assert!(!e2.is_empty());
    }

    #[tokio::test]
    async fn five_parallel_runs_all_complete() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();

        for i in 0..5 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("parallel-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }

        let mut receipts = Vec::new();
        for h in handles {
            let (_, r) = drain_run(h).await;
            receipts.push(r.unwrap());
        }

        assert_eq!(receipts.len(), 5);
        for r in &receipts {
            assert_eq!(r.outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn parallel_runs_have_distinct_run_ids() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();

        for i in 0..3 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("id-check-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }

        let ids: Vec<Uuid> = handles.iter().map(|h| h.run_id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "all run IDs should be unique");

        for h in handles {
            let _ = drain_run(h).await;
        }
    }

    #[tokio::test]
    async fn parallel_runs_produce_independent_event_streams() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "a",
            MultiEventBackend {
                name: "backend-a".into(),
                event_count: 3,
            },
        );
        rt.register_backend(
            "b",
            MultiEventBackend {
                name: "backend-b".into(),
                event_count: 5,
            },
        );

        let ha = rt
            .run_streaming("a", passthrough_wo("stream-a"))
            .await
            .unwrap();
        let hb = rt
            .run_streaming("b", passthrough_wo("stream-b"))
            .await
            .unwrap();

        let (ea, ra) = drain_run(ha).await;
        let (eb, rb) = drain_run(hb).await;

        assert!(ra.is_ok());
        assert!(rb.is_ok());
        // a: RunStarted + 3 deltas + RunCompleted = 5
        assert_eq!(ea.len(), 5);
        // b: RunStarted + 5 deltas + RunCompleted = 7
        assert_eq!(eb.len(), 7);
    }

    #[tokio::test]
    async fn concurrent_mix_of_success_and_failure() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", abp_integrations::MockBackend);
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "boom".into(),
            },
        );

        let h_ok = rt
            .run_streaming("mock", passthrough_wo("ok"))
            .await
            .unwrap();
        let h_fail = rt
            .run_streaming("fail", passthrough_wo("fail"))
            .await
            .unwrap();

        let (_, r_ok) = drain_run(h_ok).await;
        let (_, r_fail) = drain_run(h_fail).await;

        assert!(r_ok.is_ok());
        assert!(r_fail.is_err());
    }

    #[tokio::test]
    async fn ten_parallel_runs_no_data_races() {
        let rt = Runtime::with_default_backends();
        let mut join_handles = Vec::new();

        for i in 0..10 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("race-{i}")))
                .await
                .unwrap();
            join_handles.push(tokio::spawn(async move {
                let (events, receipt) = drain_run(h).await;
                (events, receipt)
            }));
        }

        let mut results = Vec::new();
        for jh in join_handles {
            let (events, receipt) = jh.await.unwrap();
            results.push((events, receipt));
        }

        for (events, receipt) in &results {
            assert!(!events.is_empty());
            assert!(receipt.is_ok());
        }
    }

    #[tokio::test]
    async fn concurrent_runs_each_get_receipt_hash() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();

        for i in 0..4 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("hash-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }

        let mut hashes = Vec::new();
        for h in handles {
            let (_, r) = drain_run(h).await;
            let receipt = r.unwrap();
            assert!(receipt.receipt_sha256.is_some());
            hashes.push(receipt.receipt_sha256.unwrap());
        }

        // All hashes should be valid hex
        for hash in &hashes {
            assert_eq!(hash.len(), 64);
        }
    }

    #[tokio::test]
    async fn metrics_track_concurrent_runs() {
        let rt = Runtime::with_default_backends();
        let initial = rt.metrics().snapshot().total_runs;

        let mut handles = Vec::new();
        for i in 0..3 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("metrics-{i}")))
                .await
                .unwrap();
            handles.push(h);
        }

        for h in handles {
            let _ = drain_run(h).await;
        }

        let final_runs = rt.metrics().snapshot().total_runs;
        assert_eq!(final_runs - initial, 3);
    }

    #[tokio::test]
    async fn receipt_chain_accumulates_concurrent_runs() {
        let rt = Runtime::with_default_backends();

        for i in 0..3 {
            let h = rt
                .run_streaming("mock", passthrough_wo(&format!("chain-{i}")))
                .await
                .unwrap();
            let _ = drain_run(h).await;
        }

        let chain = rt.receipt_chain();
        let locked = chain.lock().await;
        assert!(locked.len() >= 3);
    }

    #[tokio::test]
    async fn parallel_different_backends_complete_independently() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", abp_integrations::MockBackend);
        rt.register_backend(
            "scenario",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "scenario ok".into(),
            }),
        );

        let h1 = rt
            .run_streaming("mock", passthrough_wo("diff-backend-1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("scenario", passthrough_wo("diff-backend-2"))
            .await
            .unwrap();

        let (_, r1) = drain_run(h1).await;
        let (_, r2) = drain_run(h2).await;

        let r1 = r1.unwrap();
        let r2 = r2.unwrap();
        assert_eq!(r1.backend.id, "mock");
        assert_eq!(r2.backend.id, "scenario-mock");
    }
}
