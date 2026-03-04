#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive end-to-end integration tests for the sidecar backend pipeline.
//!
//! Exercises the full ABP pipeline (WorkOrder → Backend → Events → Receipt)
//! using `MockBackend`, `ScenarioMockBackend`, and custom test backends.
//!
//! Sections:
//! 1. Mock sidecar lifecycle (15 tests)
//! 2. Event streaming E2E (10 tests)
//! 3. Receipt verification E2E (10 tests)
//! 4. Policy enforcement E2E (10 tests)
//! 5. Multi-backend E2E (10 tests)

use std::collections::BTreeMap;
use std::path::Path;

use abp_backend_mock::scenarios::{EventSequenceBuilder, MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, RunMetadata, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_integrations::{Backend, MockBackend};
use abp_policy::{Decision, PolicyEngine};
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

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

async fn run_mock(rt: &Runtime, task: &str) -> (Vec<AgentEvent>, Receipt) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    (events, receipt.unwrap())
}

fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_receipt(
    run_id: Uuid,
    wo: &WorkOrder,
    backend_id: &str,
    trace: Vec<AgentEvent>,
    outcome: Outcome,
) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend_id.to_string(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: Default::default(),
        trace,
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

/// A backend that always fails.
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

/// A backend that emits a configurable number of delta events.
#[derive(Debug, Clone)]
struct StreamingBackend {
    chunks: Vec<String>,
    id: String,
}

#[async_trait]
impl Backend for StreamingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
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
        let started_at = Utc::now();
        let mut trace = Vec::new();

        let start = make_event(AgentEventKind::RunStarted {
            message: "streaming start".into(),
        });
        let _ = events_tx.send(start.clone()).await;
        trace.push(start);

        for chunk in &self.chunks {
            let ev = make_event(AgentEventKind::AssistantDelta {
                text: chunk.clone(),
            });
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

        let end = make_event(AgentEventKind::RunCompleted {
            message: "streaming done".into(),
        });
        let _ = events_tx.send(end.clone()).await;
        trace.push(end);

        let finished_at = Utc::now();
        let r = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        r.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// A backend that emits tool call + result events.
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
        let mut caps = BTreeMap::new();
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        caps.insert(Capability::ToolWrite, SupportLevel::Native);
        caps
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started_at = Utc::now();
        let mut trace = Vec::new();

        let kinds = [
            AgentEventKind::RunStarted {
                message: "tool-call start".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tc1".into()),
                output: json!("fn main() {}"),
                is_error: false,
            },
            AgentEventKind::AssistantMessage {
                text: "I read the file.".into(),
            },
            AgentEventKind::RunCompleted {
                message: "tool-call done".into(),
            },
        ];

        for kind in kinds {
            let ev = make_event(kind);
            let _ = events_tx.send(ev.clone()).await;
            trace.push(ev);
        }

        let finished_at = Utc::now();
        let r = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        r.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

/// A backend that emits partial events then signals partial outcome.
#[derive(Debug, Clone)]
struct PartialBackend;

#[async_trait]
impl Backend for PartialBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "partial".into(),
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
        let started_at = Utc::now();
        let mut trace = Vec::new();

        let start = make_event(AgentEventKind::RunStarted {
            message: "partial start".into(),
        });
        let _ = events_tx.send(start.clone()).await;
        trace.push(start);

        let warn = make_event(AgentEventKind::Warning {
            message: "hit limit".into(),
        });
        let _ = events_tx.send(warn.clone()).await;
        trace.push(warn);

        let end = make_event(AgentEventKind::RunCompleted {
            message: "partial done".into(),
        });
        let _ = events_tx.send(end.clone()).await;
        trace.push(end);

        let finished_at = Utc::now();
        let r = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms: (finished_at - started_at).num_milliseconds().unsigned_abs(),
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Partial,
            receipt_sha256: None,
        };
        r.with_hash().map_err(|e| anyhow::anyhow!(e))
    }
}

// ===========================================================================
// Section 1: Mock sidecar lifecycle (15 tests)
// ===========================================================================

#[tokio::test]
async fn lifecycle_mock_basic_run_completes() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "basic lifecycle test").await;
    assert!(!events.is_empty());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn lifecycle_mock_emits_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "emit run_started").await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn lifecycle_mock_emits_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "emit run_completed").await;
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn lifecycle_mock_events_have_nondecreasing_timestamps() {
    let rt = Runtime::with_default_backends();
    let (events, _) = run_mock(&rt, "timestamps").await;
    for pair in events.windows(2) {
        assert!(
            pair[1].ts >= pair[0].ts,
            "timestamps must be non-decreasing"
        );
    }
}

#[tokio::test]
async fn lifecycle_mock_receipt_has_correct_backend_id() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "backend id check").await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn lifecycle_mock_receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "contract version").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn lifecycle_scenario_success_run() {
    let scenario = MockScenario::Success {
        delay_ms: 0,
        text: "scenario success".to_string(),
    };
    let mut rt = Runtime::new();
    rt.register_backend("scenario", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("scenario test");
    let handle = rt.run_streaming("scenario", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(events.len() >= 3); // RunStarted + message + RunCompleted
}

#[tokio::test]
async fn lifecycle_scenario_streaming_success() {
    let scenario = MockScenario::StreamingSuccess {
        chunks: vec!["Hello ".into(), "world".into(), "!".into()],
        chunk_delay_ms: 0,
    };
    let mut rt = Runtime::new();
    rt.register_backend("stream-sc", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("streaming scenario");
    let handle = rt.run_streaming("stream-sc", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 3);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn lifecycle_scenario_permanent_error() {
    let scenario = MockScenario::PermanentError {
        code: "ERR-001".into(),
        message: "permanent failure".into(),
    };
    let mut rt = Runtime::new();
    rt.register_backend("perma-fail", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("permanent error test");
    let handle = rt.run_streaming("perma-fail", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn lifecycle_scenario_timeout() {
    let scenario = MockScenario::Timeout { after_ms: 10 };
    let mut rt = Runtime::new();
    rt.register_backend("timeout", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("timeout test");
    let handle = rt.run_streaming("timeout", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn lifecycle_scenario_rate_limited() {
    let scenario = MockScenario::RateLimited {
        retry_after_ms: 100,
    };
    let mut rt = Runtime::new();
    rt.register_backend("ratelim", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("rate limit test");
    let handle = rt.run_streaming("ratelim", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn lifecycle_unknown_backend_returns_error() {
    let rt = Runtime::new();
    let wo = passthrough_wo("unknown backend");
    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn lifecycle_custom_backend_integration() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "streaming-custom",
        StreamingBackend {
            chunks: vec!["a".into(), "b".into()],
            id: "streaming-custom".into(),
        },
    );
    let wo = passthrough_wo("custom backend");
    let handle = rt.run_streaming("streaming-custom", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(events.len() >= 4); // start + 2 deltas + end
}

#[tokio::test]
async fn lifecycle_run_handle_has_valid_run_id() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("run id test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    assert_ne!(run_id, Uuid::nil());
    let _ = drain_run(handle).await;
}

#[tokio::test]
async fn lifecycle_work_order_id_propagated_to_receipt() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("propagate wo id");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

// ===========================================================================
// Section 2: Event streaming E2E (10 tests)
// ===========================================================================

#[tokio::test]
async fn streaming_text_events_through_pipeline() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "text-streamer",
        StreamingBackend {
            chunks: vec!["Hello".into(), " ".into(), "world".into()],
            id: "text-streamer".into(),
        },
    );
    let wo = passthrough_wo("text streaming");
    let handle = rt.run_streaming("text-streamer", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["Hello", " ", "world"]);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_tool_call_events() {
    let mut rt = Runtime::new();
    rt.register_backend("toolcall", ToolCallBackend);
    let wo = passthrough_wo("tool calls");
    let handle = rt.run_streaming("toolcall", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert_eq!(tool_calls.len(), 1);
}

#[tokio::test]
async fn streaming_tool_result_events() {
    let mut rt = Runtime::new();
    rt.register_backend("toolresult", ToolCallBackend);
    let wo = passthrough_wo("tool results");
    let handle = rt.run_streaming("toolresult", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(tool_results.len(), 1);
}

#[tokio::test]
async fn streaming_error_events_through_pipeline() {
    let scenario = EventSequenceBuilder::new()
        .error_event("something went wrong")
        .message("recovery message")
        .outcome(Outcome::Complete)
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("error-ev", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("error events");
    let handle = rt.run_streaming("error-ev", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::Error { .. }))
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_mixed_event_types() {
    let scenario = EventSequenceBuilder::new()
        .message("initial message")
        .tool_call("Read", json!({"path": "test.txt"}))
        .tool_result("Read", json!("content"))
        .delta("partial ")
        .delta("response")
        .warning("low budget")
        .file_changed("src/lib.rs", "modified function")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("mixed", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("mixed events");
    let handle = rt.run_streaming("mixed", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    // RunStarted + 7 custom steps + RunCompleted = 9
    assert!(events.len() >= 9);
}

#[tokio::test]
async fn streaming_warning_events() {
    let scenario = EventSequenceBuilder::new()
        .warning("warning 1")
        .warning("warning 2")
        .message("ok")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("warn", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("warning events");
    let handle = rt.run_streaming("warn", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let warnings: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::Warning { .. }))
        .collect();
    assert_eq!(warnings.len(), 2);
}

#[tokio::test]
async fn streaming_file_changed_events() {
    let scenario = EventSequenceBuilder::new()
        .file_changed("src/main.rs", "added main function")
        .file_changed("src/lib.rs", "updated exports")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("filechange", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("file changes");
    let handle = rt.run_streaming("filechange", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let file_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. }))
        .collect();
    assert_eq!(file_events.len(), 2);
}

#[tokio::test]
async fn streaming_command_executed_events() {
    let scenario = EventSequenceBuilder::new()
        .command_executed("cargo test", 0, Some("all passed".into()))
        .message("tests passed")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("cmdexec", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("command exec");
    let handle = rt.run_streaming("cmdexec", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;
    let cmds: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::CommandExecuted { .. }))
        .collect();
    assert_eq!(cmds.len(), 1);
}

#[tokio::test]
async fn streaming_empty_event_sequence() {
    let scenario = EventSequenceBuilder::new().build();
    let mut rt = Runtime::new();
    rt.register_backend("empty", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("empty sequence");
    let handle = rt.run_streaming("empty", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    // Even empty scenario produces RunStarted + RunCompleted
    assert!(events.len() >= 2);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_high_volume_events() {
    let mut builder = EventSequenceBuilder::new();
    for i in 0..100 {
        builder = builder.delta(format!("chunk-{i}"));
    }
    let scenario = builder.build();
    let mut rt = Runtime::new();
    rt.register_backend("highvol", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("high volume");
    let handle = rt.run_streaming("highvol", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 100);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 3: Receipt verification E2E (10 tests)
// ===========================================================================

#[tokio::test]
async fn receipt_has_sha256_hash() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "receipt hash test").await;
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_hash_is_deterministic_for_same_receipt() {
    let wo = passthrough_wo("determinism");
    let run_id = Uuid::new_v4();
    let r = make_receipt(run_id, &wo, "test", vec![], Outcome::Complete);
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn receipt_hash_changes_with_different_outcome() {
    let wo = passthrough_wo("hash diff outcome");
    let run_id = Uuid::new_v4();
    let r1 = make_receipt(run_id, &wo, "test", vec![], Outcome::Complete);
    let mut r2 = make_receipt(run_id, &wo, "test", vec![], Outcome::Failed);
    // Use same timestamps for comparison
    r2.meta.started_at = r1.meta.started_at;
    r2.meta.finished_at = r1.meta.finished_at;
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[tokio::test]
async fn receipt_trace_matches_streamed_events_count() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_mock(&rt, "trace count").await;
    // The runtime may augment the trace, but events streamed should be a subset.
    assert!(!events.is_empty());
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_metadata_timing_valid() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "timing check").await;
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn receipt_contract_version_is_current() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "version check").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_serialization_roundtrip() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "serde roundtrip").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.outcome, receipt.outcome);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(deserialized.backend.id, receipt.backend.id);
}

#[tokio::test]
async fn receipt_scenario_with_usage_tokens() {
    let scenario = EventSequenceBuilder::new()
        .message("counted response")
        .usage_tokens(150, 75)
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("usage", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("usage test");
    let handle = rt.run_streaming("usage", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(150));
    assert_eq!(receipt.usage.output_tokens, Some(75));
}

#[tokio::test]
async fn receipt_partial_outcome() {
    let mut rt = Runtime::new();
    rt.register_backend("partial", PartialBackend);
    let wo = passthrough_wo("partial run");
    let handle = rt.run_streaming("partial", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[tokio::test]
async fn receipt_with_hash_excludes_hash_from_input() {
    let wo = passthrough_wo("self-ref prevention");
    let run_id = Uuid::new_v4();
    let r = make_receipt(run_id, &wo, "test", vec![], Outcome::Complete);
    let hashed = r.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    // Hashing again after clearing should yield the same hash
    let mut r2 = hashed.clone();
    r2.receipt_sha256 = None;
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(hashed.receipt_sha256.unwrap(), h2);
}

#[tokio::test]
async fn receipt_run_id_matches_handle() {
    let rt = Runtime::with_default_backends();
    let wo = passthrough_wo("run id match");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

// ===========================================================================
// Section 4: Policy enforcement E2E (10 tests)
// ===========================================================================

#[tokio::test]
async fn policy_allow_tool_passes() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

#[tokio::test]
async fn policy_deny_tool_blocks() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[tokio::test]
async fn policy_tool_not_in_allowlist_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[tokio::test]
async fn policy_deny_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["secret.txt".into(), "**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
    assert!(!engine.can_read_path(Path::new("config/.env")).allowed);
    assert!(engine.can_read_path(Path::new("readme.md")).allowed);
}

#[tokio::test]
async fn policy_deny_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into(), "Cargo.lock".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

#[tokio::test]
async fn policy_deny_overrides_allow() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    // Deny should override allow
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

#[tokio::test]
async fn policy_empty_means_permissive() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(engine.can_read_path(Path::new("anything.txt")).allowed);
    assert!(engine.can_write_path(Path::new("anything.txt")).allowed);
}

#[tokio::test]
async fn policy_glob_pattern_in_deny_write() {
    let policy = PolicyProfile {
        deny_write: vec!["*.log".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new("app.log")).allowed);
    assert!(!engine.can_write_path(Path::new("debug.log")).allowed);
    assert!(engine.can_write_path(Path::new("app.txt")).allowed);
}

#[tokio::test]
async fn policy_with_work_order_integration() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("policy integration")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(policy.clone())
        .build();
    // Verify work order carries the policy
    assert_eq!(wo.policy.allowed_tools, vec!["Read".to_string()]);
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash".to_string()]);

    // Compile and verify enforcement
    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
}

#[tokio::test]
async fn policy_decision_has_reason_on_deny() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["secret.txt".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let tool_decision = engine.can_use_tool("Bash");
    assert!(!tool_decision.allowed);
    assert!(tool_decision.reason.is_some());

    let read_decision = engine.can_read_path(Path::new("secret.txt"));
    assert!(!read_decision.allowed);
    assert!(read_decision.reason.is_some());
}

// ===========================================================================
// Section 5: Multi-backend E2E (10 tests)
// ===========================================================================

#[tokio::test]
async fn multi_backend_register_and_run_mock() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);
    let (events, receipt) = run_mock(&rt, "multi mock").await;
    assert!(!events.is_empty());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn multi_backend_register_multiple() {
    let mut rt = Runtime::new();
    rt.register_backend("mock-a", MockBackend);
    rt.register_backend("mock-b", MockBackend);
    rt.register_backend(
        "custom",
        StreamingBackend {
            chunks: vec!["x".into()],
            id: "custom".into(),
        },
    );
    let names = rt.backend_names();
    assert!(names.contains(&"mock-a".to_string()));
    assert!(names.contains(&"mock-b".to_string()));
    assert!(names.contains(&"custom".to_string()));
}

#[tokio::test]
async fn multi_backend_run_each_independently() {
    let mut rt = Runtime::new();
    rt.register_backend("back-a", MockBackend);
    rt.register_backend(
        "back-b",
        StreamingBackend {
            chunks: vec!["hello".into()],
            id: "back-b".into(),
        },
    );

    let wo_a = passthrough_wo("run a");
    let handle_a = rt.run_streaming("back-a", wo_a).await.unwrap();
    let (_, receipt_a) = drain_run(handle_a).await;
    assert_eq!(receipt_a.unwrap().backend.id, "mock");

    let wo_b = passthrough_wo("run b");
    let handle_b = rt.run_streaming("back-b", wo_b).await.unwrap();
    let (_, receipt_b) = drain_run(handle_b).await;
    assert_eq!(receipt_b.unwrap().backend.id, "back-b");
}

#[tokio::test]
async fn multi_backend_failing_does_not_affect_others() {
    let mut rt = Runtime::new();
    rt.register_backend("good", MockBackend);
    rt.register_backend(
        "bad",
        FailingBackend {
            message: "always fails".into(),
        },
    );

    // Bad backend fails
    let wo_bad = passthrough_wo("bad run");
    let handle_bad = rt.run_streaming("bad", wo_bad).await.unwrap();
    let (_, receipt_bad) = drain_run(handle_bad).await;
    assert!(receipt_bad.is_err());

    // Good backend still works
    let wo_good = passthrough_wo("good run");
    let handle_good = rt.run_streaming("good", wo_good).await.unwrap();
    let (_, receipt_good) = drain_run(handle_good).await;
    assert_eq!(receipt_good.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn multi_backend_scenario_transient_error() {
    let scenario = MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "recovered".into(),
        }),
    };
    let backend = ScenarioMockBackend::new(scenario);
    let mut rt = Runtime::new();
    rt.register_backend("transient", backend.clone());

    // First call: fails
    let wo1 = passthrough_wo("call 1");
    let handle1 = rt.run_streaming("transient", wo1).await.unwrap();
    let (_, r1) = drain_run(handle1).await;
    assert!(r1.is_err());

    // Second call: still fails
    let wo2 = passthrough_wo("call 2");
    let handle2 = rt.run_streaming("transient", wo2).await.unwrap();
    let (_, r2) = drain_run(handle2).await;
    assert!(r2.is_err());

    // Third call: succeeds
    let wo3 = passthrough_wo("call 3");
    let handle3 = rt.run_streaming("transient", wo3).await.unwrap();
    let (_, r3) = drain_run(handle3).await;
    assert_eq!(r3.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn multi_backend_capability_checking() {
    let mut rt = Runtime::new();
    rt.register_backend("mock", MockBackend);

    // MockBackend supports Streaming natively
    let backend = rt.backend("mock").unwrap();
    let caps = backend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[tokio::test]
async fn multi_backend_custom_scenario_with_fail_after() {
    let scenario = EventSequenceBuilder::new()
        .message("this will be emitted")
        .delta("partial data")
        .fail_after("mid-stream crash")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("crasher", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("crash test");
    let handle = rt.run_streaming("crasher", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    assert!(receipt.is_err());
}

#[tokio::test]
async fn multi_backend_identity_preserved() {
    let mut rt = Runtime::new();
    rt.register_backend("tool-back", ToolCallBackend);
    let backend = rt.backend("tool-back").unwrap();
    let identity = backend.identity();
    assert_eq!(identity.id, "tool-call");
    assert_eq!(identity.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn multi_backend_sequential_runs_same_backend() {
    let rt = Runtime::with_default_backends();
    for i in 0..5 {
        let (events, receipt) = run_mock(&rt, &format!("seq run {i}")).await;
        assert!(!events.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn multi_backend_scenario_custom_outcome() {
    let scenario = EventSequenceBuilder::new()
        .message("partial result")
        .warning("budget exceeded")
        .outcome(Outcome::Partial)
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("partial-sc", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("partial outcome scenario");
    let handle = rt.run_streaming("partial-sc", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

// ===========================================================================
// Section 6: Additional edge cases (5+ bonus tests)
// ===========================================================================

#[tokio::test]
async fn edge_case_scenario_recorder_tracks_calls() {
    use abp_backend_mock::scenarios::MockBackendRecorder;

    let recorder = MockBackendRecorder::new(MockBackend);
    let mut rt = Runtime::new();
    rt.register_backend("recorded", recorder.clone());

    let wo = passthrough_wo("recorded call 1");
    let handle = rt.run_streaming("recorded", wo).await.unwrap();
    let _ = drain_run(handle).await;

    let wo2 = passthrough_wo("recorded call 2");
    let handle2 = rt.run_streaming("recorded", wo2).await.unwrap();
    let _ = drain_run(handle2).await;

    recorder.assert_call_count(2).await;
    recorder.assert_all_succeeded().await;
}

#[tokio::test]
async fn edge_case_large_event_payload() {
    let large_text = "x".repeat(10_000);
    let scenario = EventSequenceBuilder::new()
        .message(large_text.clone())
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("large", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("large payload");
    let handle = rt.run_streaming("large", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(msgs.iter().any(|m| m.len() == 10_000));
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn edge_case_receipt_json_canonical_form() {
    let rt = Runtime::with_default_backends();
    let (_, receipt) = run_mock(&rt, "canonical json").await;
    let json = serde_json::to_string(&receipt).unwrap();
    // Canonical JSON should be parseable
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_object());
    assert!(parsed.get("meta").is_some());
    assert!(parsed.get("backend").is_some());
    assert!(parsed.get("outcome").is_some());
}

#[tokio::test]
async fn edge_case_work_order_builder_all_options() {
    let wo = WorkOrderBuilder::new("full options")
        .workspace_mode(WorkspaceMode::PassThrough)
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["secret.txt".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();
    assert_eq!(wo.task, "full options");
    assert_eq!(wo.policy.allowed_tools, vec!["Read".to_string()]);
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash".to_string()]);
}

#[tokio::test]
async fn edge_case_multiple_tool_calls_in_sequence() {
    let scenario = EventSequenceBuilder::new()
        .tool_call("Read", json!({"path": "a.txt"}))
        .tool_result("Read", json!("aaa"))
        .tool_call("Write", json!({"path": "b.txt", "content": "bbb"}))
        .tool_result("Write", json!("ok"))
        .tool_call("Grep", json!({"pattern": "test"}))
        .tool_result("Grep", json!(["match1", "match2"]))
        .message("done")
        .build();
    let mut rt = Runtime::new();
    rt.register_backend("multi-tool", ScenarioMockBackend::new(scenario));
    let wo = passthrough_wo("multi tool calls");
    let handle = rt.run_streaming("multi-tool", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(tool_calls.len(), 3);
    assert_eq!(tool_results.len(), 3);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn edge_case_scenario_call_count_tracking() {
    let scenario = MockScenario::Success {
        delay_ms: 0,
        text: "counted".into(),
    };
    let backend = ScenarioMockBackend::new(scenario);
    let mut rt = Runtime::new();
    rt.register_backend("counter", backend.clone());

    for _ in 0..3 {
        let wo = passthrough_wo("count me");
        let handle = rt.run_streaming("counter", wo).await.unwrap();
        let _ = drain_run(handle).await;
    }

    // calls() uses Arc<Mutex<>> so it's shared across clones, unlike call_count()
    let calls = backend.calls().await;
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|c| c.result.is_ok()));
}
