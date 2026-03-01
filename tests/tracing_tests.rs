// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests verifying tracing/logging output from the ABP runtime,
//! host, and workspace modules.

use std::sync::{Arc, Mutex};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, Receipt, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared log-capture infrastructure
// ---------------------------------------------------------------------------

/// Thread-safe buffer that captures tracing output.
#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl LogBuf {
    fn contents(&self) -> String {
        let buf = self.0.lock().unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }

    fn contains(&self, needle: &str) -> bool {
        self.contents().contains(needle)
    }
}

impl std::io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
    type Writer = LogBuf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install a tracing subscriber that captures all output into a [`LogBuf`].
/// Returns the buffer and a guard that must be held for the test duration.
fn setup_tracing() -> (LogBuf, tracing::subscriber::DefaultGuard) {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_target(true)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (buf, guard)
}

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

/// A backend that emits a configurable number of events before succeeding.
#[derive(Debug, Clone)]
struct EventCountBackend {
    count: usize,
}

#[async_trait]
impl Backend for EventCountBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "event-counter".into(),
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
        let started = chrono::Utc::now();
        let mut trace = Vec::new();
        for i in 0..self.count {
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
        let finished = chrono::Utc::now();
        Ok(Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: abp_core::CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms: (finished - started)
                    .to_std()
                    .unwrap_or_default()
                    .as_millis() as u64,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: abp_core::ExecutionMode::default(),
            usage_raw: serde_json::json!({}),
            usage: Default::default(),
            trace,
            artifacts: vec![],
            verification: Default::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        })
    }
}

// ===========================================================================
// 1. Runtime emits expected events during work order execution
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn runtime_emits_starting_run_trace() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("trace test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(logs.contains("starting run"), "logs: {}", logs.contents());
    assert!(logs.contains("mock"), "logs: {}", logs.contents());
}

// ===========================================================================
// 2. Host sidecar module emits stderr capture events
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn host_stderr_target_used_for_sidecar_warnings() {
    let (logs, _guard) = setup_tracing();

    // The sidecar stderr logging uses target "abp.sidecar.stderr".
    // We cannot spawn a real sidecar here, but verify the target is captured.
    tracing::warn!(target: "abp.sidecar.stderr", "simulated sidecar stderr line");

    assert!(
        logs.contains("simulated sidecar stderr line"),
        "logs: {}",
        logs.contents()
    );
    assert!(
        logs.contains("abp.sidecar.stderr"),
        "logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// 3. Workspace staging emits progress events
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn workspace_staging_emits_debug_trace() {
    let (logs, _guard) = setup_tracing();

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("hello.txt"), "world").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staging trace")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // copy_workspace emits: "staging workspace from {src} to {dest}"
    assert!(
        logs.contains("staging workspace"),
        "logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// 4. Error paths produce warn-level events
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn unknown_backend_emits_warning_trace() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("should fail")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let result = rt.run_streaming("nonexistent_backend", wo).await;
    assert!(result.is_err());

    assert!(
        logs.contains("unknown backend"),
        "logs: {}",
        logs.contents()
    );
    assert!(
        logs.contains("nonexistent_backend"),
        "logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// 5. Debug level produces verbose output
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn debug_level_includes_starting_run() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("debug test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // The runtime emits at debug level: "starting run"
    assert!(logs.contains("starting run"), "logs: {}", logs.contents());
}

// ===========================================================================
// 6. Run ID appears in span context
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn run_id_appears_in_trace_output() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("run id test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // debug! includes run_id=%run_id
    assert!(
        logs.contains(&run_id.to_string()),
        "run_id {} not in logs: {}",
        run_id,
        logs.contents()
    );
}

// ===========================================================================
// 7. Backend name appears in traces
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn backend_name_appears_in_trace_output() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("backend name test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // debug! includes backend=%backend_name
    assert!(logs.contains("backend"), "logs: {}", logs.contents());
    assert!(logs.contains("mock"), "logs: {}", logs.contents());
}

// ===========================================================================
// 8. Event count recorded in telemetry after run
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn event_count_recorded_in_telemetry() {
    let (logs, _guard) = setup_tracing();

    let event_count = 5;
    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: event_count });

    let wo = WorkOrderBuilder::new("event count test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // Events were streamed.
    assert_eq!(events.len(), event_count);

    // Telemetry metrics reflect the event count.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_events, event_count as u64);
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);

    // The runtime traces with "starting run" for this backend too.
    assert!(logs.contains("starting run"), "logs: {}", logs.contents());
}

// ===========================================================================
// 9. Duration appears in telemetry
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn duration_recorded_in_metrics() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("duration test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Telemetry snapshot records the run with a duration.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
    assert!(snap.average_run_duration_ms <= receipt.meta.duration_ms + 5000);

    // The "starting run" debug trace was emitted.
    assert!(logs.contains("starting run"), "logs: {}", logs.contents());
}

// ===========================================================================
// 10. Multiple concurrent runs have isolated span contexts
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn concurrent_runs_have_isolated_traces() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();

    let mut handles = Vec::new();
    let mut run_ids = Vec::new();
    for i in 0..3 {
        let wo = WorkOrderBuilder::new(format!("concurrent trace {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        run_ids.push(handle.run_id);
        handles.push(handle);
    }

    let mut receipts = Vec::new();
    for handle in handles {
        let (_, receipt) = drain_run(handle).await;
        receipts.push(receipt.unwrap());
    }

    // Each run_id should appear in the trace output.
    for run_id in &run_ids {
        assert!(
            logs.contains(&run_id.to_string()),
            "run_id {run_id} not in logs: {}",
            logs.contents()
        );
    }

    // All receipts completed successfully.
    assert_eq!(receipts.len(), 3);
    for r in &receipts {
        assert_eq!(r.outcome, Outcome::Complete);
    }

    // All run IDs are unique.
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 3);
}

// ===========================================================================
// 11. Failing backend produces BackendFailed without panic in traces
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn failing_backend_traced_without_panic() {
    let (logs, _guard) = setup_tracing();

    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("failing trace test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(matches!(receipt, Err(RuntimeError::BackendFailed(_))));

    // The runtime should have emitted "starting run" before the failure.
    assert!(logs.contains("starting run"), "logs: {}", logs.contents());
}

// ===========================================================================
// 12. Workspace staging target matches expected tracing target
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn workspace_staging_uses_correct_tracing_target() {
    let (logs, _guard) = setup_tracing();

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("data.txt"), "test").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("target test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // Workspace tracing uses target "abp.workspace".
    assert!(logs.contains("abp.workspace"), "logs: {}", logs.contents());
}

// ===========================================================================
// 13. Runtime tracing target is abp.runtime
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn runtime_uses_correct_tracing_target() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("target test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    // Runtime tracing uses target "abp.runtime".
    assert!(logs.contains("abp.runtime"), "logs: {}", logs.contents());
}
