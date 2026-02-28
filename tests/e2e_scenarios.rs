// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end scenario tests simulating real-world usage patterns.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest,
    ContextPacket, ContextSnippet, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
    receipt_hash, filter::EventFilter,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::PolicyEngine;
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

/// A custom backend that emits tool-call / tool-result events to simulate
/// multi-step agent interactions.
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
// 1. Code review scenario
// ===========================================================================

#[tokio::test]
async fn scenario_code_review() {
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new("Review the login module for security issues")
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ContextPacket {
            files: vec!["src/auth/login.rs".into()],
            snippets: vec![ContextSnippet {
                name: "review-guidelines".into(),
                content: "Check for SQL injection and XSS vulnerabilities".into(),
            }],
        })
        .model("gpt-4")
        .build();

    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.receipt_sha256.is_some());
    assert!(!events.is_empty());

    // Verify receipt hash is correct.
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());

    // Run ID is propagated.
    assert_eq!(receipt.meta.run_id, run_id);
}

// ===========================================================================
// 2. Multi-step task with tool calls
// ===========================================================================

#[tokio::test]
async fn scenario_multi_step_tool_calls() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-call-mock", ToolCallBackend);

    let wo = WorkOrderBuilder::new("Read main.rs, edit it, then build")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("tool-call-mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);

    // Verify we got the tool call/result pairs.
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(tool_calls.len(), 3, "expected 3 tool calls");
    assert_eq!(tool_results.len(), 3, "expected 3 tool results");

    // Verify tool call order: Read → Edit → Bash.
    let tool_names: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(tool_names, vec!["Read", "Edit", "Bash"]);

    // Verify file change event.
    let file_changes: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::FileChanged { .. }))
        .collect();
    assert_eq!(file_changes.len(), 1);

    // Receipt trace matches streamed events.
    assert_eq!(receipt.trace.len(), events.len());
}

// ===========================================================================
// 3. Agent handoff — second work order references first receipt
// ===========================================================================

#[tokio::test]
async fn scenario_agent_handoff() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    // First run: initial task.
    let wo1 = WorkOrderBuilder::new("Analyze the codebase structure")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let wo1_id = wo1.id;
    let handle1 = rt.run_streaming("mock", wo1).await.unwrap();
    let (_, receipt1) = drain_run(handle1).await;
    let receipt1 = receipt1.unwrap();
    store.save(&receipt1).unwrap();

    // Second run: references first work order via context.
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

    // Both receipts are loadable and verifiable.
    assert!(store.verify(receipt1.meta.run_id).unwrap());
    assert!(store.verify(receipt2.meta.run_id).unwrap());

    // They have distinct run IDs but both completed.
    assert_ne!(receipt1.meta.run_id, receipt2.meta.run_id);
    assert_eq!(receipt1.outcome, Outcome::Complete);
    assert_eq!(receipt2.outcome, Outcome::Complete);

    // Chain is valid.
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid);
    assert_eq!(chain.valid_count, 2);
}

// ===========================================================================
// 4. Policy restricted agent — strict read-only
// ===========================================================================

#[tokio::test]
async fn scenario_policy_restricted_agent() {
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

    // Allowed tools pass.
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Glob").allowed);
    assert!(engine.can_use_tool("Grep").allowed);

    // Disallowed tools blocked.
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Edit").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("WebFetch").allowed);

    // Read paths: normal files OK, secrets blocked.
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("secrets/api_key.txt")).allowed);

    // Write paths: everything blocked.
    assert!(!engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_write_path(Path::new("README.md")).allowed);

    // Full pipeline still completes (MockBackend doesn't actually call tools).
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Read-only audit of codebase")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().outcome, Outcome::Complete);
}

// ===========================================================================
// 5. Workspace isolation — two concurrent work orders
// ===========================================================================

#[tokio::test]
async fn scenario_workspace_isolation() {
    let dir_a = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("file_a.txt"), "workspace A content").unwrap();

    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_b.path().join("file_b.txt"), "workspace B content").unwrap();

    let rt = Runtime::with_default_backends();

    let wo_a = WorkOrderBuilder::new("Task in workspace A")
        .root(dir_a.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let wo_b = WorkOrderBuilder::new("Task in workspace B")
        .root(dir_b.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    // Run concurrently.
    let handle_a = rt.run_streaming("mock", wo_a).await.unwrap();
    let handle_b = rt.run_streaming("mock", wo_b).await.unwrap();

    let (events_a, receipt_a) = drain_run(handle_a).await;
    let (events_b, receipt_b) = drain_run(handle_b).await;

    let receipt_a = receipt_a.unwrap();
    let receipt_b = receipt_b.unwrap();

    // Both completed independently.
    assert_eq!(receipt_a.outcome, Outcome::Complete);
    assert_eq!(receipt_b.outcome, Outcome::Complete);

    // Different run IDs.
    assert_ne!(receipt_a.meta.run_id, receipt_b.meta.run_id);

    // Both produced events.
    assert!(!events_a.is_empty());
    assert!(!events_b.is_empty());

    // Both have valid hashes.
    assert_eq!(
        receipt_a.receipt_sha256.as_deref(),
        Some(receipt_hash(&receipt_a).unwrap().as_str())
    );
    assert_eq!(
        receipt_b.receipt_sha256.as_deref(),
        Some(receipt_hash(&receipt_b).unwrap().as_str())
    );
}

// ===========================================================================
// 6. Backend selection — register multiple, route correctly
// ===========================================================================

#[tokio::test]
async fn scenario_backend_selection() {
    let mut rt = Runtime::new();
    rt.register_backend("mock-alpha", MockBackend);
    rt.register_backend("mock-beta", MockBackend);
    rt.register_backend("tool-backend", ToolCallBackend);

    // Verify all registered.
    let names = rt.backend_names();
    assert!(names.contains(&"mock-alpha".to_string()));
    assert!(names.contains(&"mock-beta".to_string()));
    assert!(names.contains(&"tool-backend".to_string()));

    // Route to mock-alpha.
    let wo1 = WorkOrderBuilder::new("task for alpha")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle1 = rt.run_streaming("mock-alpha", wo1).await.unwrap();
    let (_, r1) = drain_run(handle1).await;
    let r1 = r1.unwrap();
    assert_eq!(r1.backend.id, "mock");
    assert_eq!(r1.outcome, Outcome::Complete);

    // Route to tool-backend.
    let wo2 = WorkOrderBuilder::new("task for tool backend")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle2 = rt.run_streaming("tool-backend", wo2).await.unwrap();
    let (events2, r2) = drain_run(handle2).await;
    let r2 = r2.unwrap();
    assert_eq!(r2.backend.id, "tool-call-mock");
    assert_eq!(r2.outcome, Outcome::Complete);

    // Tool backend emits tool calls, mock doesn't.
    let tool_calls_from_tool: Vec<_> = events2
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert!(!tool_calls_from_tool.is_empty());

    // Unknown backend fails.
    let wo3 = WorkOrderBuilder::new("task for unknown")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    match rt.run_streaming("nonexistent", wo3).await {
        Err(RuntimeError::UnknownBackend { name }) => assert_eq!(name, "nonexistent"),
        Err(e) => panic!("expected UnknownBackend, got {e:?}"),
        Ok(_) => panic!("expected UnknownBackend, got Ok"),
    }
}

// ===========================================================================
// 7. Telemetry flow — verify metrics after runs
// ===========================================================================

#[tokio::test]
async fn scenario_telemetry_flow() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("failing", FailingBackend);

    assert_eq!(rt.metrics().snapshot().total_runs, 0);
    assert_eq!(rt.metrics().snapshot().total_events, 0);

    // Run two successful tasks.
    for i in 0..2 {
        let wo = WorkOrderBuilder::new(format!("telemetry task {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        receipt.unwrap();
    }

    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 2);
    assert_eq!(snap.failed_runs, 0);
    assert!(snap.total_events > 0);
    assert!(snap.average_run_duration_ms < 10_000); // sanity

    // Run a failing task.
    let wo = WorkOrderBuilder::new("will fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());

    // Metrics don't count runs that errored at the backend level
    // (BackendFailed is returned before metrics.record_run is called).
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 2);
}

// ===========================================================================
// 8. Config-driven pipeline
// ===========================================================================

#[tokio::test]
async fn scenario_config_driven_pipeline() {
    use abp_cli::config::{BackendConfig, BackplaneConfig};
    use std::collections::HashMap;

    // Build a config programmatically (simulating parsed backplane.toml).
    let config = BackplaneConfig {
        default_backend: None,
        log_level: None,
        receipts_dir: None,
        backends: HashMap::from([
            ("cfg-mock-1".into(), BackendConfig::Mock {}),
            ("cfg-mock-2".into(), BackendConfig::Mock {}),
        ]),
    };

    // Validate config.
    abp_cli::config::validate_config(&config).unwrap();

    // Register backends from config.
    let mut rt = Runtime::new();
    for (name, bc) in &config.backends {
        match bc {
            BackendConfig::Mock {} => {
                rt.register_backend(name.as_str(), MockBackend);
            }
            BackendConfig::Sidecar { .. } => {
                // Would register SidecarBackend in real usage.
            }
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

// ===========================================================================
// 9. Receipt audit trail — 5 sequential runs
// ===========================================================================

#[tokio::test]
async fn scenario_receipt_audit_trail() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());
    let rt = Runtime::with_default_backends();

    let mut receipts = Vec::new();

    for i in 0..5 {
        let wo = WorkOrderBuilder::new(format!("audit trail step {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        store.save(&receipt).unwrap();
        receipts.push(receipt);
    }

    // All 5 receipts stored.
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 5);

    // Every receipt is individually verifiable.
    for r in &receipts {
        assert!(store.verify(r.meta.run_id).unwrap());
        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.outcome, Outcome::Complete);
        assert_eq!(loaded.meta.contract_version, CONTRACT_VERSION);
    }

    // Chain verification passes.
    let chain = store.verify_chain().unwrap();
    assert!(chain.is_valid, "chain should be valid");
    assert_eq!(chain.valid_count, 5);
    assert!(chain.invalid_hashes.is_empty());
    assert_eq!(chain.gaps.len(), 4); // 5 runs → 4 gaps

    // All run IDs are unique.
    let unique_ids: std::collections::HashSet<_> =
        receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(unique_ids.len(), 5);

    // Hashes are deterministic: recomputing matches stored.
    for r in &receipts {
        let recomputed = receipt_hash(r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref(), Some(recomputed.as_str()));
    }
}

// ===========================================================================
// 10. Event filtering
// ===========================================================================

#[tokio::test]
async fn scenario_event_filtering() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-mock", ToolCallBackend);

    let wo = WorkOrderBuilder::new("task with event filtering")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("tool-mock", wo).await.unwrap();
    let (all_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!all_events.is_empty());

    // Include filter: only tool calls.
    let include_filter = EventFilter::include_kinds(&["tool_call"]);
    let tool_only: Vec<_> = all_events
        .iter()
        .filter(|e| include_filter.matches(e))
        .collect();
    assert_eq!(tool_only.len(), 3);
    for ev in &tool_only {
        assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
    }

    // Exclude filter: remove assistant messages.
    let exclude_filter = EventFilter::exclude_kinds(&["assistant_message", "assistant_delta"]);
    let no_assistant: Vec<_> = all_events
        .iter()
        .filter(|e| exclude_filter.matches(e))
        .collect();
    // All assistant messages should be gone.
    for ev in &no_assistant {
        assert!(!matches!(
            ev.kind,
            AgentEventKind::AssistantMessage { .. } | AgentEventKind::AssistantDelta { .. }
        ));
    }
    assert!(no_assistant.len() < all_events.len());

    // Include filter: only lifecycle events.
    let lifecycle_filter = EventFilter::include_kinds(&["run_started", "run_completed"]);
    let lifecycle: Vec<_> = all_events
        .iter()
        .filter(|e| lifecycle_filter.matches(e))
        .collect();
    assert_eq!(lifecycle.len(), 2);
}

// ===========================================================================
// 11. Large payload — 100KB task description
// ===========================================================================

#[tokio::test]
async fn scenario_large_payload() {
    let large_task = "x".repeat(100 * 1024); // 100 KB
    let rt = Runtime::with_default_backends();

    let wo = WorkOrderBuilder::new(large_task.clone())
        .workspace_mode(WorkspaceMode::PassThrough)
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "big-snippet".into(),
                content: "y".repeat(50 * 1024), // 50 KB snippet
            }],
        })
        .build();
    assert_eq!(wo.task.len(), 100 * 1024);
    assert_eq!(wo.context.snippets[0].content.len(), 50 * 1024);

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());

    // Hash is still valid.
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(hash, &receipt_hash(&receipt).unwrap());

    // Receipt serializes/deserializes round-trip.
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.outcome, Outcome::Complete);
    assert_eq!(
        deserialized.receipt_sha256.as_deref(),
        receipt.receipt_sha256.as_deref()
    );
}

// ===========================================================================
// 12. Error recovery flow — first run fails, second succeeds
// ===========================================================================

#[tokio::test]
async fn scenario_error_recovery_flow() {
    let store_dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(store_dir.path());

    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);
    rt.register_backend("mock", MockBackend);

    // First run: fails.
    let wo1 = WorkOrderBuilder::new("this will fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let _wo1_id = wo1.id;
    let handle1 = rt.run_streaming("failing", wo1).await.unwrap();
    let (events1, receipt1) = drain_run(handle1).await;
    assert!(receipt1.is_err());

    // No events from a failing backend.
    assert!(events1.is_empty());

    // Second run: succeeds (retry with different backend).
    let wo2 = WorkOrderBuilder::new("retry: this will succeed")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle2 = rt.run_streaming("mock", wo2).await.unwrap();
    let (events2, receipt2) = drain_run(handle2).await;
    let receipt2 = receipt2.unwrap();

    assert_eq!(receipt2.outcome, Outcome::Complete);
    assert!(!events2.is_empty());
    store.save(&receipt2).unwrap();

    // Receipt store contains only the successful run.
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
    assert!(store.verify(receipt2.meta.run_id).unwrap());

    // Metrics: only the successful run was recorded by the runtime.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.successful_runs, 1);
}

// ===========================================================================
// 13. Passthrough vs mapped mode in config
// ===========================================================================

#[tokio::test]
async fn scenario_passthrough_mode_config() {
    let rt = Runtime::with_default_backends();

    // Default mode is Mapped.
    let wo_mapped = WorkOrderBuilder::new("mapped mode task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo_mapped).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().mode, ExecutionMode::Mapped);

    // Set passthrough mode via vendor config.
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo_pt = WorkOrderBuilder::new("passthrough mode task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let handle = rt.run_streaming("mock", wo_pt).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert_eq!(receipt.unwrap().mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// 14. Concurrent pipelines with mixed backends
// ===========================================================================

#[tokio::test]
async fn scenario_concurrent_mixed_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    rt.register_backend("tool-mock", ToolCallBackend);

    // Launch 4 runs concurrently across 2 backend types.
    let mut handles = Vec::new();
    for i in 0..4 {
        let backend = if i % 2 == 0 { "mock" } else { "tool-mock" };
        let wo = WorkOrderBuilder::new(format!("concurrent task {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        handles.push((backend, rt.run_streaming(backend, wo).await.unwrap()));
    }

    let mut receipts = Vec::new();
    for (_backend, handle) in handles {
        let (_, receipt) = drain_run(handle).await;
        receipts.push(receipt.unwrap());
    }

    assert_eq!(receipts.len(), 4);

    // All run IDs unique.
    let ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 4);

    // All completed.
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
        let hash = receipt_hash(r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref(), Some(hash.as_str()));
    }

    // Mock backends have "mock" identity, tool backends have "tool-call-mock".
    let backend_ids: Vec<_> = receipts.iter().map(|r| r.backend.id.as_str()).collect();
    assert!(backend_ids.contains(&"mock"));
    assert!(backend_ids.contains(&"tool-call-mock"));
}
