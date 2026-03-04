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
//! End-to-end scenario tests exercising the full ABP pipeline.
//!
//! Organized into modules:
//! - `happy_path` — submit tasks, receive events, verify receipts
//! - `error_scenarios` — invalid backends, failing backends, capability mismatches
//! - `multi_backend` — register and route across multiple backends
//! - `receipt_verification` — hash integrity, chain, store, validation
//! - `workspace_scenarios` — staged workspaces, passthrough, isolation

use std::collections::BTreeMap;

use abp_backend_mock::scenarios::{MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionMode,
    MinSupport, Outcome, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    receipt_hash, validate::validate_receipt,
};
use abp_integrations::{Backend, MockBackend};
use abp_runtime::store::ReceiptStore;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain all streamed events and await the receipt from a [`RunHandle`].
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

/// A backend that always returns an error, for negative-path testing.
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

/// A custom backend that emits tool-call / tool-result events.
#[derive(Debug, Clone)]
struct ToolCallBackend;

#[async_trait]
impl Backend for ToolCallBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "tool-call-mock".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        use abp_core::{Capability as C, SupportLevel as S};
        let mut m = CapabilityManifest::default();
        m.insert(C::Streaming, S::Native);
        m.insert(C::ToolRead, S::Native);
        m.insert(C::ToolWrite, S::Native);
        m.insert(C::ToolBash, S::Native);
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

        let events: Vec<AgentEventKind> = vec![
            AgentEventKind::RunStarted {
                message: format!("tool-call backend starting: {}", work_order.task),
            },
            AgentEventKind::AssistantMessage {
                text: "I'll read the file, then edit it, then run a command.".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            AgentEventKind::ToolCall {
                tool_name: "Edit".into(),
                tool_use_id: Some("tc-2".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs", "content": "fn main() { println!(\"hello\"); }"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Edit".into(),
                tool_use_id: Some("tc-2".into()),
                output: serde_json::json!({"success": true}),
                is_error: false,
            },
            AgentEventKind::ToolCall {
                tool_name: "Bash".into(),
                tool_use_id: Some("tc-3".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "cargo build"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Bash".into(),
                tool_use_id: Some("tc-3".into()),
                output: serde_json::json!({"exit_code": 0}),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added println".into(),
            },
            AgentEventKind::RunCompleted {
                message: "tool-call run complete".into(),
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
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

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
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({"note": "tool-call-mock"}),
            usage: abp_core::UsageNormalized {
                input_tokens: Some(150),
                output_tokens: Some(80),
                ..Default::default()
            },
            trace,
            artifacts: vec![],
            verification: abp_core::VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?;

        Ok(receipt)
    }
}

// ===========================================================================
// Module: happy_path
// ===========================================================================

mod happy_path {
    use super::*;

    #[tokio::test]
    async fn submit_task_receive_events_get_receipt_verify_hash() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("say hello")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let run_id = handle.run_id;
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.backend.id, "mock");
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert_eq!(receipt.meta.run_id, run_id);
        assert!(!events.is_empty());

        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash, &receipt_hash(&receipt).unwrap());
    }

    #[tokio::test]
    async fn streaming_task_receives_deltas_and_assembles_response() {
        let scenario = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
            chunks: vec!["Hello".into(), ", ".into(), "world".into(), "!".into()],
            chunk_delay_ms: 0,
        });

        let mut rt = Runtime::new();
        rt.register_backend("streamer", scenario);

        let wo = WorkOrderBuilder::new("stream test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("streamer", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);

        let deltas: Vec<String> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::AssistantDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["Hello", ", ", "world", "!"]);

        let assembled: String = deltas.join("");
        assert_eq!(assembled, "Hello, world!");
    }

    #[tokio::test]
    async fn task_with_tool_use_receives_calls_and_results() {
        let mut rt = Runtime::new();
        rt.register_backend("tools", ToolCallBackend);

        let wo = WorkOrderBuilder::new("Read, edit, build")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("tools", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);

        let tool_names: Vec<String> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_names, vec!["Read", "Edit", "Bash"]);

        let results: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
            .collect();
        assert_eq!(results.len(), 3);

        // Trace matches streamed events.
        assert_eq!(receipt.trace.len(), events.len());
    }

    #[tokio::test]
    async fn submit_task_to_specific_backend_verified_in_receipt() {
        let mut rt = Runtime::new();
        rt.register_backend("alpha", MockBackend);
        rt.register_backend("beta", ToolCallBackend);

        let wo = WorkOrderBuilder::new("route to beta")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("beta", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.backend.id, "tool-call-mock");
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn config_overrides_preserved_through_pipeline() {
        let rt = Runtime::with_default_backends();

        let mut vendor = BTreeMap::new();
        vendor.insert("custom_key".to_string(), serde_json::json!("custom_value"));
        vendor.insert(
            "abp".to_string(),
            serde_json::json!({"mode": "passthrough"}),
        );

        let wo = WorkOrderBuilder::new("config override test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .config(RuntimeConfig {
                model: Some("gpt-4-turbo".into()),
                vendor,
                max_budget_usd: Some(1.50),
                max_turns: Some(5),
                ..Default::default()
            })
            .build();

        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
        assert_eq!(wo.config.max_turns, Some(5));
        assert_eq!(wo.config.max_budget_usd, Some(1.50));
        assert!(wo.config.vendor.contains_key("custom_key"));

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        // Passthrough mode applied via vendor config.
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn context_packet_with_files_and_snippets() {
        let rt = Runtime::with_default_backends();

        let wo = WorkOrderBuilder::new("Review the auth module")
            .workspace_mode(WorkspaceMode::PassThrough)
            .context(ContextPacket {
                files: vec!["src/auth/login.rs".into(), "src/auth/token.rs".into()],
                snippets: vec![ContextSnippet {
                    name: "guidelines".into(),
                    content: "Check for injection vulnerabilities".into(),
                }],
            })
            .model("gpt-4")
            .build();

        assert_eq!(wo.context.files.len(), 2);
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn empty_config_minimal_work_order() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("minimal")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        assert!(wo.config.model.is_none());
        assert!(wo.config.max_turns.is_none());
        assert!(wo.config.max_budget_usd.is_none());
        assert!(wo.config.vendor.is_empty());
        assert!(wo.requirements.required.is_empty());
        assert!(wo.context.files.is_empty());

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(!events.is_empty());
        assert!(receipt.receipt_sha256.is_some());
        validate_receipt(&receipt).unwrap();
    }

    #[tokio::test]
    async fn large_payload_100kb_through_pipeline() {
        let large_task = "x".repeat(100 * 1024);
        let rt = Runtime::with_default_backends();

        let wo = WorkOrderBuilder::new(large_task)
            .workspace_mode(WorkspaceMode::PassThrough)
            .context(ContextPacket {
                files: vec![],
                snippets: vec![ContextSnippet {
                    name: "big".into(),
                    content: "y".repeat(50 * 1024),
                }],
            })
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, &receipt_hash(&receipt).unwrap());
    }
}

// ===========================================================================
// Module: error_scenarios
// ===========================================================================

mod error_scenarios {
    use super::*;

    #[tokio::test]
    async fn submit_to_nonexistent_backend_returns_unknown_backend() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("task for unknown")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        match rt.run_streaming("nonexistent", wo).await {
            Err(RuntimeError::UnknownBackend { name }) => assert_eq!(name, "nonexistent"),
            Ok(_) => panic!("expected UnknownBackend, got Ok"),
            Err(other) => panic!("expected UnknownBackend, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backend_fails_returns_backend_failed_error() {
        let mut rt = Runtime::new();
        rt.register_backend("failing", FailingBackend);

        let wo = WorkOrderBuilder::new("doomed task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("failing", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;

        assert!(receipt.is_err());
        match receipt.unwrap_err() {
            RuntimeError::BackendFailed(e) => {
                let chain = format!("{e:?}");
                assert!(chain.contains("intentional failure"));
            }
            other => panic!("expected BackendFailed, got {other:?}"),
        }
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn capability_requirement_not_met_returns_error() {
        let rt = Runtime::with_default_backends();

        // MockBackend does NOT support McpClient.
        let wo = WorkOrderBuilder::new("task requiring MCP")
            .workspace_mode(WorkspaceMode::PassThrough)
            .requirements(CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Native,
                }],
            })
            .build();

        match rt.run_streaming("mock", wo).await {
            Err(RuntimeError::CapabilityCheckFailed(msg)) => {
                assert!(msg.contains("McpClient"));
            }
            Ok(_) => panic!("expected CapabilityCheckFailed, got Ok"),
            Err(other) => panic!("expected CapabilityCheckFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn permanent_error_scenario_always_fails() {
        let scenario = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "ABP-E001".into(),
            message: "permanent failure".into(),
        });

        let mut rt = Runtime::new();
        rt.register_backend("perm-fail", scenario);

        let wo = WorkOrderBuilder::new("will permanently fail")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("perm-fail", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;

        assert!(receipt.is_err());
        let msg = format!("{:?}", receipt.unwrap_err());
        assert!(msg.contains("permanent failure"));
    }

    #[tokio::test]
    async fn timeout_scenario_returns_error() {
        let scenario = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 10 });

        let mut rt = Runtime::new();
        rt.register_backend("timeout", scenario);

        let wo = WorkOrderBuilder::new("will timeout")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("timeout", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;

        assert!(receipt.is_err());
        let msg = format!("{:?}", receipt.unwrap_err());
        assert!(msg.contains("timeout"));
    }

    #[tokio::test]
    async fn rate_limited_scenario_returns_error() {
        let scenario = ScenarioMockBackend::new(MockScenario::RateLimited {
            retry_after_ms: 100,
        });

        let mut rt = Runtime::new();
        rt.register_backend("rate-limited", scenario);

        let wo = WorkOrderBuilder::new("will be rate limited")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("rate-limited", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;

        assert!(receipt.is_err());
        let msg = format!("{:?}", receipt.unwrap_err());
        assert!(msg.contains("rate limited"));
    }

    #[tokio::test]
    async fn error_recovery_flow_fail_then_succeed() {
        let mut rt = Runtime::new();
        rt.register_backend("failing", FailingBackend);
        rt.register_backend("mock", MockBackend);

        // First run: fails.
        let wo1 = WorkOrderBuilder::new("this will fail")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle1 = rt.run_streaming("failing", wo1).await.unwrap();
        let (_, receipt1) = drain_run(handle1).await;
        assert!(receipt1.is_err());

        // Retry with different backend: succeeds.
        let wo2 = WorkOrderBuilder::new("retry: this will succeed")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle2 = rt.run_streaming("mock", wo2).await.unwrap();
        let (events2, receipt2) = drain_run(handle2).await;
        let receipt2 = receipt2.unwrap();

        assert_eq!(receipt2.outcome, Outcome::Complete);
        assert!(!events2.is_empty());
    }

    #[tokio::test]
    async fn runtime_error_has_error_codes() {
        let unknown = RuntimeError::UnknownBackend { name: "foo".into() };
        assert_eq!(unknown.error_code(), abp_error::ErrorCode::BackendNotFound);
        assert!(!unknown.is_retryable());

        let be = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
        assert_eq!(be.error_code(), abp_error::ErrorCode::BackendCrashed);
        assert!(be.is_retryable());
    }
}

// ===========================================================================
// Module: multi_backend
// ===========================================================================

mod multi_backend {
    use super::*;

    #[tokio::test]
    async fn register_multiple_mock_backends_each_handles_requests() {
        let mut rt = Runtime::new();
        rt.register_backend("mock-alpha", MockBackend);
        rt.register_backend("mock-beta", MockBackend);
        rt.register_backend("tool-mock", ToolCallBackend);

        let names = rt.backend_names();
        assert!(names.contains(&"mock-alpha".to_string()));
        assert!(names.contains(&"mock-beta".to_string()));
        assert!(names.contains(&"tool-mock".to_string()));

        for backend_name in &["mock-alpha", "mock-beta", "tool-mock"] {
            let wo = WorkOrderBuilder::new(format!("task for {backend_name}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt.run_streaming(backend_name, wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
        }
    }

    #[tokio::test]
    async fn backend_identity_differs_per_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("tools", ToolCallBackend);

        let wo1 = WorkOrderBuilder::new("mock task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle1 = rt.run_streaming("mock", wo1).await.unwrap();
        let (_, r1) = drain_run(handle1).await;

        let wo2 = WorkOrderBuilder::new("tool task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle2 = rt.run_streaming("tools", wo2).await.unwrap();
        let (_, r2) = drain_run(handle2).await;

        assert_eq!(r1.unwrap().backend.id, "mock");
        assert_eq!(r2.unwrap().backend.id, "tool-call-mock");
    }

    #[tokio::test]
    async fn concurrent_runs_across_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("tools", ToolCallBackend);

        let mut handles = Vec::new();
        for i in 0..4 {
            let backend = if i % 2 == 0 { "mock" } else { "tools" };
            let wo = WorkOrderBuilder::new(format!("concurrent task {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            handles.push(rt.run_streaming(backend, wo).await.unwrap());
        }

        let mut ids = std::collections::HashSet::new();
        for handle in handles {
            let (_, receipt) = drain_run(handle).await;
            let r = receipt.unwrap();
            assert_eq!(r.outcome, Outcome::Complete);
            ids.insert(r.meta.run_id);
        }
        assert_eq!(ids.len(), 4, "all run IDs must be unique");
    }

    #[tokio::test]
    async fn five_concurrent_runs_all_succeed() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();

        for i in 0..5 {
            let wo = WorkOrderBuilder::new(format!("parallel task {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            handles.push(rt.run_streaming("mock", wo).await.unwrap());
        }

        let mut run_ids = std::collections::HashSet::new();
        for handle in handles {
            let (events, receipt) = drain_run(handle).await;
            let receipt = receipt.unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert!(!events.is_empty());
            run_ids.insert(receipt.meta.run_id);
        }
        assert_eq!(run_ids.len(), 5);
    }

    #[tokio::test]
    async fn tool_backend_emits_tool_events_mock_does_not() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("tools", ToolCallBackend);

        let wo_mock = WorkOrderBuilder::new("mock task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle_mock = rt.run_streaming("mock", wo_mock).await.unwrap();
        let (events_mock, _) = drain_run(handle_mock).await;

        let wo_tools = WorkOrderBuilder::new("tools task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle_tools = rt.run_streaming("tools", wo_tools).await.unwrap();
        let (events_tools, _) = drain_run(handle_tools).await;

        let mock_tool_calls: Vec<_> = events_mock
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
            .collect();
        let tool_tool_calls: Vec<_> = events_tools
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
            .collect();

        assert!(mock_tool_calls.is_empty());
        assert_eq!(tool_tool_calls.len(), 3);
    }

    #[tokio::test]
    async fn telemetry_metrics_across_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("failing", FailingBackend);

        assert_eq!(rt.metrics().snapshot().total_runs, 0);

        // Two successful runs.
        for i in 0..2 {
            let wo = WorkOrderBuilder::new(format!("task {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            receipt.unwrap();
        }

        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 2);
        assert_eq!(snap.successful_runs, 2);
        assert!(snap.total_events > 0);
    }
}

// ===========================================================================
// Module: receipt_verification
// ===========================================================================

mod receipt_verification {
    use super::*;
    use abp_core::chain::ReceiptChain;

    #[tokio::test]
    async fn receipt_hash_matches_after_full_run() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("hash verify")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        let stored_hash = receipt.receipt_sha256.as_ref().unwrap();
        let recomputed = receipt_hash(&receipt).unwrap();
        assert_eq!(stored_hash, &recomputed);
        assert_eq!(stored_hash.len(), 64);
    }

    #[tokio::test]
    async fn receipt_contract_version_correct() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("version check")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert_eq!(receipt.meta.contract_version, "abp/v0.1");

        // JSON round-trip preserves version.
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("abp/v0.1"));
        let loaded: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn receipt_outcome_reflects_actual_result() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("outcome check")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.meta.duration_ms < 30_000);
    }

    #[tokio::test]
    async fn receipt_store_save_load_verify() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(store_dir.path());
        let rt = Runtime::with_default_backends();

        let wo = WorkOrderBuilder::new("store test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        let original_hash = receipt.receipt_sha256.clone().unwrap();

        let path = store.save(&receipt).unwrap();
        assert!(path.exists());

        let loaded = store.load(receipt.meta.run_id).unwrap();
        assert_eq!(
            loaded.receipt_sha256.as_deref(),
            Some(original_hash.as_str())
        );

        let recomputed = receipt_hash(&loaded).unwrap();
        assert_eq!(recomputed, original_hash);
        assert!(store.verify(receipt.meta.run_id).unwrap());

        validate_receipt(&loaded).unwrap();
    }

    #[tokio::test]
    async fn receipt_chain_five_runs_verify_integrity() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(store_dir.path());
        let rt = Runtime::with_default_backends();

        let mut receipts = Vec::new();
        for i in 0..5 {
            let wo = WorkOrderBuilder::new(format!("chain step {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            let receipt = receipt.unwrap();
            store.save(&receipt).unwrap();
            receipts.push(receipt);
        }

        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 5);

        for r in &receipts {
            assert!(store.verify(r.meta.run_id).unwrap());
            let loaded = store.load(r.meta.run_id).unwrap();
            assert_eq!(loaded.outcome, Outcome::Complete);
            assert_eq!(loaded.meta.contract_version, CONTRACT_VERSION);
        }

        let chain = store.verify_chain().unwrap();
        assert!(chain.is_valid);
        assert_eq!(chain.valid_count, 5);
        assert!(chain.invalid_hashes.is_empty());

        let unique_ids: std::collections::HashSet<_> =
            receipts.iter().map(|r| r.meta.run_id).collect();
        assert_eq!(unique_ids.len(), 5);

        for r in &receipts {
            let recomputed = receipt_hash(r).unwrap();
            assert_eq!(r.receipt_sha256.as_deref(), Some(recomputed.as_str()));
        }
    }

    #[tokio::test]
    async fn receipt_chain_struct_verify() {
        let rt = Runtime::with_default_backends();
        let mut chain = ReceiptChain::new();

        for i in 0..3 {
            let wo = WorkOrderBuilder::new(format!("chain item {i}"))
                .workspace_mode(WorkspaceMode::PassThrough)
                .build();
            let handle = rt.run_streaming("mock", wo).await.unwrap();
            let (_, receipt) = drain_run(handle).await;
            chain.push(receipt.unwrap()).unwrap();
        }

        assert_eq!(chain.len(), 3);
        chain.verify().unwrap();
        assert!((chain.success_rate() - 1.0).abs() < f64::EPSILON);
        assert!(chain.total_events() > 0);
        assert_eq!(chain.find_by_backend("mock").len(), 3);
    }
}

// ===========================================================================
// Module: workspace_scenarios
// ===========================================================================

mod workspace_scenarios {
    use super::*;

    #[tokio::test]
    async fn task_with_staged_workspace_produces_git_verification() {
        let src_dir = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
        std::fs::write(src_dir.path().join("src").join("lib.rs"), "// lib").unwrap();

        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("refactor staged workspace")
            .root(src_dir.path().to_str().unwrap())
            .workspace_mode(WorkspaceMode::Staged)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.verification.git_diff.is_some());
        assert!(receipt.verification.git_status.is_some());
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn task_without_workspace_passthrough_mode() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("passthrough task")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(!events.is_empty());
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn workspace_isolation_two_concurrent_workspaces() {
        let dir_a = tempfile::tempdir().unwrap();
        std::fs::write(dir_a.path().join("file_a.txt"), "workspace A").unwrap();

        let dir_b = tempfile::tempdir().unwrap();
        std::fs::write(dir_b.path().join("file_b.txt"), "workspace B").unwrap();

        let rt = Runtime::with_default_backends();

        let wo_a = WorkOrderBuilder::new("Task A")
            .root(dir_a.path().to_str().unwrap())
            .workspace_mode(WorkspaceMode::Staged)
            .build();
        let wo_b = WorkOrderBuilder::new("Task B")
            .root(dir_b.path().to_str().unwrap())
            .workspace_mode(WorkspaceMode::Staged)
            .build();

        let handle_a = rt.run_streaming("mock", wo_a).await.unwrap();
        let handle_b = rt.run_streaming("mock", wo_b).await.unwrap();

        let (_, receipt_a) = drain_run(handle_a).await;
        let (_, receipt_b) = drain_run(handle_b).await;

        let receipt_a = receipt_a.unwrap();
        let receipt_b = receipt_b.unwrap();

        assert_eq!(receipt_a.outcome, Outcome::Complete);
        assert_eq!(receipt_b.outcome, Outcome::Complete);
        assert_ne!(receipt_a.meta.run_id, receipt_b.meta.run_id);

        assert_eq!(
            receipt_a.receipt_sha256.as_deref(),
            Some(receipt_hash(&receipt_a).unwrap().as_str())
        );
        assert_eq!(
            receipt_b.receipt_sha256.as_deref(),
            Some(receipt_hash(&receipt_b).unwrap().as_str())
        );
    }

    #[tokio::test]
    async fn staged_workspace_with_include_exclude_globs() {
        let src_dir = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("keep.rs"), "fn keep() {}").unwrap();
        std::fs::write(src_dir.path().join("skip.log"), "log data").unwrap();

        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("glob filter test")
            .root(src_dir.path().to_str().unwrap())
            .workspace_mode(WorkspaceMode::Staged)
            .include(vec!["*.rs".into()])
            .exclude(vec!["*.log".into()])
            .build();

        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.receipt_sha256.is_some());
    }
}

// ===========================================================================
// Remaining flat tests (preserved from original for coverage)
// ===========================================================================

#[tokio::test]
async fn scenario_agent_handoff() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let wo1 = WorkOrderBuilder::new("Analyze the codebase structure")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let wo1_id = wo1.id;
    let handle1 = rt.run_streaming("mock", wo1).await.unwrap();
    let (_, receipt1) = drain_run(handle1).await;
    let receipt1 = receipt1.unwrap();
    store.save(&receipt1).unwrap();

    let wo2 = WorkOrderBuilder::new("Based on prior analysis, refactor the auth module")
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "prior-run".into(),
                content: format!(
                    "Previous work order {} completed with run {}",
                    wo1_id, receipt1.meta.run_id
                ),
            }],
        })
        .build();
    let handle2 = rt.run_streaming("mock", wo2).await.unwrap();
    let (_, receipt2) = drain_run(handle2).await;
    let receipt2 = receipt2.unwrap();
    store.save(&receipt2).unwrap();

    assert!(store.verify(receipt1.meta.run_id).unwrap());
    assert!(store.verify(receipt2.meta.run_id).unwrap());
    assert_ne!(receipt1.meta.run_id, receipt2.meta.run_id);
    assert_eq!(receipt1.outcome, Outcome::Complete);
    assert_eq!(receipt2.outcome, Outcome::Complete);

    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 2);
}

#[tokio::test]
async fn scenario_policy_restricted_agent() {
    use abp_core::PolicyProfile;
    use abp_policy::PolicyEngine;
    use std::path::Path;

    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
        disallowed_tools: vec![
            "Write".into(),
            "Edit".into(),
            "Bash".into(),
            "WebFetch".into(),
        ],
        deny_read: vec![
            "**/.env".into(),
            "**/.env.*".into(),
            "**/secrets/**".into(),
            "**/id_rsa".into(),
        ],
        deny_write: vec!["**/*".into()],
        allow_network: vec![],
        deny_network: vec!["*".into()],
        require_approval_for: vec![],
    };

    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_write_path(Path::new("src/main.rs")).allowed);

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Read-only audit of codebase")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_config_driven_pipeline() {
    use abp_cli::config::{BackendConfig, BackplaneConfig};
    use std::collections::HashMap;

    let config = BackplaneConfig {
        default_backend: None,
        log_level: None,
        receipts_dir: None,
        backends: HashMap::from([
            ("cfg-mock-1".into(), BackendConfig::Mock {}),
            ("cfg-mock-2".into(), BackendConfig::Mock {}),
        ]),
    };

    abp_cli::config::validate_config(&config).unwrap();

    let mut rt = Runtime::new();
    for (name, bc) in &config.backends {
        match bc {
            BackendConfig::Mock {} => {
                rt.register_backend(name.as_str(), MockBackend);
            }
            BackendConfig::Sidecar { .. } => {}
        }
    }

    // Verify backends registered.
    assert!(rt.backend("cfg-mock-1").is_some());
    assert!(rt.backend("cfg-mock-2").is_some());

    // Run a work order against a config-registered backend.
    let wo = WorkOrderBuilder::new("config-driven task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            model: Some("test-model".into()),
            max_turns: Some(10),
            max_budget_usd: Some(0.5),
            ..Default::default()
        })
        .build();

    let handle = rt.run_streaming("cfg-mock-1", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
}
