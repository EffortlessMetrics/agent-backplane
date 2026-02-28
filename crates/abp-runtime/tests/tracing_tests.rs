// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests that verify structured tracing output from the runtime.
//!
//! Uses a capturing subscriber to collect formatted log lines, then asserts on
//! their content.  The `.with_subscriber()` combinator from `tracing` ensures
//! that spawned tasks inherit the test subscriber.

use abp_core::{
    CapabilityRequirements, ExecutionLane, PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use tracing::Instrument;
use tracing_subscriber::fmt::MakeWriter;

// ---------------------------------------------------------------------------
// Capturing infrastructure
// ---------------------------------------------------------------------------

/// Shared buffer that implements `io::Write` + `MakeWriter` so
/// `tracing_subscriber::fmt` can write formatted events into it.
#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedLogs;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl CapturedLogs {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }

    fn contains(&self, needle: &str) -> bool {
        self.contents().contains(needle)
    }
}

/// Build a subscriber that captures all levels and returns the log buffer.
fn capturing_subscriber() -> (tracing::subscriber::DefaultGuard, CapturedLogs) {
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (guard, logs)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "tracing test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

/// Run a work order to completion, draining all events and awaiting the receipt.
async fn run_to_completion(rt: &Runtime, wo: WorkOrder) -> abp_core::Receipt {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    handle.receipt.await.expect("join").expect("receipt")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------- 1. Runtime run produces at least one trace event ----------

#[tokio::test(flavor = "current_thread")]
async fn runtime_run_emits_trace_event() {
    let (_guard, logs) = capturing_subscriber();
    let rt = Runtime::with_default_backends();
    run_to_completion(&rt, mock_work_order())
        .instrument(tracing::info_span!("test"))
        .await;

    assert!(
        logs.contains("starting run"),
        "expected 'starting run' in logs, got:\n{}",
        logs.contents()
    );
}

// ---------- 2. Backend name appears in trace context ----------

#[tokio::test(flavor = "current_thread")]
async fn trace_contains_backend_name() {
    let (_guard, logs) = capturing_subscriber();
    let rt = Runtime::with_default_backends();
    run_to_completion(&rt, mock_work_order())
        .instrument(tracing::info_span!("test"))
        .await;

    // The runtime emits: debug!(target: "abp.runtime", backend=%backend_name, ...)
    assert!(
        logs.contains("mock"),
        "expected backend name 'mock' in logs, got:\n{}",
        logs.contents()
    );
}

// ---------- 3. Error conditions produce warning-level traces ----------

#[tokio::test(flavor = "current_thread")]
async fn unknown_backend_emits_warning() {
    let (_guard, logs) = capturing_subscriber();
    let rt = Runtime::with_default_backends();

    let result = rt.run_streaming("nonexistent_backend", mock_work_order()).await;
    assert!(result.is_err(), "should fail for unknown backend");

    let output = logs.contents();
    assert!(
        output.contains("unknown backend"),
        "expected 'unknown backend' in logs, got:\n{output}"
    );
    assert!(
        output.contains("nonexistent_backend"),
        "expected 'nonexistent_backend' in logs, got:\n{output}"
    );
    assert!(
        output.contains("WARN"),
        "expected WARN level in logs, got:\n{output}"
    );
}

// ---------- 4. Staged workspace logs appropriate events ----------

#[tokio::test(flavor = "current_thread")]
async fn staged_workspace_emits_trace() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::fs::write(tmp.path().join("file.txt"), "content").expect("write file");

    let (_guard, logs) = capturing_subscriber();
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.workspace = WorkspaceSpec {
        root: tmp.path().to_string_lossy().into_owned(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    let _receipt = handle.receipt.await.expect("join").expect("receipt");

    // abp-workspace emits: debug!(target: "abp.workspace", "staging workspace from ...")
    assert!(
        logs.contains("staging workspace"),
        "expected 'staging workspace' in logs, got:\n{}",
        logs.contents()
    );
}

// ---------- 5. Successful run does not produce warning-level traces ----------

#[tokio::test(flavor = "current_thread")]
async fn successful_run_has_no_warnings() {
    let (_guard, logs) = capturing_subscriber();
    let rt = Runtime::with_default_backends();
    run_to_completion(&rt, mock_work_order())
        .instrument(tracing::info_span!("test"))
        .await;

    assert!(
        !logs.contains("unknown backend"),
        "successful run should not contain warnings, got:\n{}",
        logs.contents()
    );
}
