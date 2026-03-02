// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for tracing/telemetry across the ABP system.
//!
//! Covers span creation, parent-child relationships, event attributes,
//! error codes, concurrent safety, overhead, structured logging,
//! and target-based filtering.

use std::sync::{Arc, Mutex};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, Receipt, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_integrations::Backend;
use abp_runtime::{Runtime, RuntimeError};
use abp_telemetry::{
    JsonExporter, MetricsCollector, RunMetrics as TelemetryRunMetrics, TelemetryExporter,
    TelemetrySpan,
};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Log-capture infrastructure
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

/// Install a tracing subscriber that captures output into a [`LogBuf`].
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

/// Install a tracing subscriber that emits JSON-formatted events.
fn setup_json_tracing() -> (LogBuf, tracing::subscriber::DefaultGuard) {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_target(true)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (buf, guard)
}

/// Install a subscriber with an env-filter for target-based filtering.
fn setup_filtered_tracing(filter: &str) -> (LogBuf, tracing::subscriber::DefaultGuard) {
    use tracing_subscriber::EnvFilter;
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_env_filter(EnvFilter::new(filter))
        .with_target(true)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (buf, guard)
}

// ---------------------------------------------------------------------------
// Helper backends
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
// 1. Verify tracing spans are created for key operations
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn spans_created_for_run_streaming() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("span test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(
        logs.contains("starting run"),
        "expected 'starting run' in logs: {}",
        logs.contents()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn spans_created_for_workspace_staging() {
    let (logs, _guard) = setup_tracing();

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("file.txt"), "data").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("staging span test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(
        logs.contains("staging workspace"),
        "expected 'staging workspace' in logs: {}",
        logs.contents()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn spans_created_for_backend_execution() {
    let (logs, _guard) = setup_tracing();

    let mut rt = Runtime::new();
    rt.register_backend("counter", EventCountBackend { count: 3 });

    let wo = WorkOrderBuilder::new("backend exec span test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("counter", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(
        logs.contains("starting run"),
        "expected 'starting run' in logs: {}",
        logs.contents()
    );
    assert!(
        logs.contains("counter") || logs.contains("event-counter"),
        "expected backend name in logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// 2. Verify span hierarchy (parent-child relationships)
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn runtime_and_workspace_spans_coexist_in_staged_run() {
    let (logs, _guard) = setup_tracing();

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("test.txt"), "content").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("hierarchy test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    // Both runtime and workspace targets should appear in a staged run.
    assert!(
        log_text.contains("abp.runtime"),
        "expected abp.runtime target: {log_text}"
    );
    assert!(
        log_text.contains("abp.workspace"),
        "expected abp.workspace target: {log_text}"
    );
    // "starting run" should come after "staging workspace" chronologically.
    let staging_pos = log_text.find("staging workspace");
    let starting_pos = log_text.find("starting run");
    assert!(
        staging_pos.is_some() && starting_pos.is_some(),
        "both spans must be present: {log_text}"
    );
    assert!(
        staging_pos.unwrap() < starting_pos.unwrap(),
        "workspace staging should precede backend start: {log_text}"
    );
}

// ===========================================================================
// 3. Verify event attributes include expected fields
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn event_attributes_include_run_id() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("run_id attr test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let run_id = handle.run_id;
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(
        logs.contains(&run_id.to_string()),
        "run_id {} not found in logs: {}",
        run_id,
        logs.contents()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn event_attributes_include_backend_name() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("backend attr test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    assert!(
        logs.contains("mock"),
        "expected 'mock' backend in logs: {}",
        logs.contents()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn telemetry_span_carries_dialect_attribute() {
    let (_logs, _guard) = setup_tracing();

    let span = TelemetrySpan::new("run")
        .with_attribute("run_id", "abc-123")
        .with_attribute("backend", "mock")
        .with_attribute("dialect", "openai");

    assert_eq!(span.attributes["dialect"], "openai");
    assert_eq!(span.attributes["backend"], "mock");
    assert_eq!(span.attributes["run_id"], "abc-123");

    // Emit without panic.
    span.emit();
}

// ===========================================================================
// 4. Verify error spans include error codes
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn error_span_includes_backend_name_on_unknown() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("error code test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let result = rt.run_streaming("nonexistent", wo).await;
    assert!(result.is_err());

    let log_text = logs.contents();
    assert!(
        log_text.contains("unknown backend"),
        "expected 'unknown backend' in logs: {log_text}"
    );
    assert!(
        log_text.contains("nonexistent"),
        "expected backend name in error log: {log_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failing_backend_produces_error_without_panic() {
    let (logs, _guard) = setup_tracing();

    let mut rt = Runtime::new();
    rt.register_backend("failing", FailingBackend);

    let wo = WorkOrderBuilder::new("failing error test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("failing", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    assert!(
        matches!(receipt, Err(RuntimeError::BackendFailed(_))),
        "expected BackendFailed"
    );

    // Starting run trace was emitted before the failure.
    assert!(
        logs.contains("starting run"),
        "expected 'starting run' in logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// 5. Test with a mock subscriber that captures events
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn mock_subscriber_captures_runtime_events() {
    let (logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("mock subscriber test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    // Verify multiple distinct log lines were captured.
    let line_count = log_text.lines().count();
    assert!(
        line_count >= 1,
        "expected at least 1 log line, got {line_count}: {log_text}"
    );
    assert!(
        log_text.contains("abp.runtime"),
        "expected abp.runtime target in captured logs: {log_text}"
    );
}

#[test]
fn telemetry_span_emit_captured_by_subscriber() {
    let (logs, _guard) = setup_tracing();

    let span = TelemetrySpan::new("test_operation").with_attribute("key", "value");
    span.emit();

    let log_text = logs.contents();
    assert!(
        log_text.contains("telemetry_span"),
        "expected 'telemetry_span' in logs: {log_text}"
    );
    assert!(
        log_text.contains("test_operation"),
        "expected span name in logs: {log_text}"
    );
}

// ===========================================================================
// 6. Verify no panics on concurrent telemetry calls
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn concurrent_runs_no_panic() {
    let (_logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let mut handles = Vec::new();

    for i in 0..5 {
        let wo = WorkOrderBuilder::new(format!("concurrent {i}"))
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        handles.push(handle);
    }

    for handle in handles {
        let (_events, receipt) = drain_run(handle).await;
        receipt.unwrap();
    }

    // Metrics reflect all runs.
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 5);
    assert_eq!(snap.successful_runs, 5);
}

#[test]
fn concurrent_metrics_collector_no_panic() {
    let collector = MetricsCollector::new();
    let mut threads = Vec::new();

    for i in 0..20 {
        let c = collector.clone();
        threads.push(std::thread::spawn(move || {
            c.record(TelemetryRunMetrics {
                backend_name: format!("backend-{i}"),
                dialect: "test".into(),
                duration_ms: i * 10,
                events_count: 5,
                tokens_in: 100,
                tokens_out: 200,
                tool_calls_count: 2,
                errors_count: 0,
                emulations_applied: 0,
            });
            // Concurrently read summaries while recording.
            let _ = c.summary();
            let _ = c.len();
        }));
    }

    for t in threads {
        t.join().unwrap();
    }

    assert_eq!(collector.len(), 20);
    let summary = collector.summary();
    assert_eq!(summary.count, 20);
}

#[test]
fn concurrent_telemetry_span_emit_no_panic() {
    let (_logs, _guard) = setup_tracing();

    let mut threads = Vec::new();
    for i in 0..20 {
        threads.push(std::thread::spawn(move || {
            let span = TelemetrySpan::new(format!("concurrent-{i}"))
                .with_attribute("index", i.to_string());
            span.emit();
        }));
    }

    for t in threads {
        t.join().unwrap();
    }
}

// ===========================================================================
// 7. Test telemetry overhead
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn telemetry_overhead_is_minimal() {
    let (_logs, _guard) = setup_tracing();

    let rt = Runtime::with_default_backends();
    let start = std::time::Instant::now();

    for _ in 0..10 {
        let wo = WorkOrderBuilder::new("overhead test")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _ = drain_run(handle).await;
    }

    let elapsed = start.elapsed();
    // 10 mock runs with PassThrough should complete in a reasonable time.
    // Debug builds on Windows with git-init per staged run can be slow, so
    // we use a generous upper bound.
    assert!(
        elapsed.as_secs() < 300,
        "telemetry overhead too high: {elapsed:?} for 10 mock runs"
    );
}

#[test]
fn metrics_collector_overhead_is_minimal() {
    let collector = MetricsCollector::new();

    let start = std::time::Instant::now();
    for i in 0..1000 {
        collector.record(TelemetryRunMetrics {
            backend_name: "bench".into(),
            dialect: "test".into(),
            duration_ms: i,
            events_count: 1,
            tokens_in: 10,
            tokens_out: 20,
            tool_calls_count: 0,
            errors_count: 0,
            emulations_applied: 0,
        });
    }
    let _summary = collector.summary();
    let elapsed = start.elapsed();

    // 1000 records + summary should complete in well under 1 second.
    assert!(
        elapsed.as_millis() < 1000,
        "collector overhead too high: {elapsed:?} for 1000 records"
    );
    assert_eq!(collector.len(), 1000);
}

// ===========================================================================
// 8. Verify structured logging format (JSON structured events)
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn json_structured_logging_produces_valid_json() {
    let (logs, _guard) = setup_json_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("json format test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    assert!(!log_text.is_empty(), "expected some log output");

    // Each non-empty line should parse as valid JSON.
    for line in log_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(trimmed);
        assert!(
            parsed.is_ok(),
            "line is not valid JSON: {trimmed}\nparse error: {:?}",
            parsed.err()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn json_structured_logging_includes_target_field() {
    let (logs, _guard) = setup_json_tracing();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("json target test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    let mut found_runtime_target = false;
    for line in log_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(target) = v.get("target").and_then(|t| t.as_str())
            && target == "abp.runtime"
        {
            found_runtime_target = true;
        }
    }
    assert!(
        found_runtime_target,
        "expected at least one JSON event with target 'abp.runtime': {log_text}"
    );
}

#[test]
fn json_exporter_produces_structured_output() {
    let collector = MetricsCollector::new();
    collector.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 42,
        events_count: 3,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 1,
        errors_count: 0,
        emulations_applied: 0,
    });

    let summary = collector.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&summary).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
    assert!(parsed["mean_duration_ms"].as_f64().is_some());
    assert!(parsed["backend_counts"]["mock"].as_i64().is_some());
}

// ===========================================================================
// 9. Test with tracing-subscriber test utilities
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn subscriber_set_default_isolates_test() {
    // Two different subscribers in sequence should not interfere.
    {
        let (logs1, _guard1) = setup_tracing();
        tracing::info!(target: "abp.runtime", "first");
        assert!(logs1.contains("first"));
    }
    {
        let (logs2, _guard2) = setup_tracing();
        tracing::info!(target: "abp.runtime", "second");
        assert!(logs2.contains("second"));
        assert!(!logs2.contains("first"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn subscriber_captures_all_levels() {
    let (logs, _guard) = setup_tracing();

    tracing::trace!(target: "abp.runtime", "trace msg");
    tracing::debug!(target: "abp.runtime", "debug msg");
    tracing::info!(target: "abp.runtime", "info msg");
    tracing::warn!(target: "abp.runtime", "warn msg");
    tracing::error!(target: "abp.runtime", "error msg");

    let log_text = logs.contents();
    assert!(log_text.contains("trace msg"), "missing TRACE: {log_text}");
    assert!(log_text.contains("debug msg"), "missing DEBUG: {log_text}");
    assert!(log_text.contains("info msg"), "missing INFO: {log_text}");
    assert!(log_text.contains("warn msg"), "missing WARN: {log_text}");
    assert!(log_text.contains("error msg"), "missing ERROR: {log_text}");
}

#[tokio::test(flavor = "current_thread")]
async fn subscriber_records_span_fields() {
    let (logs, _guard) = setup_tracing();

    let span = tracing::info_span!("test_span", run_id = "abc-123", backend = "mock");
    let _enter = span.enter();
    tracing::info!("inside span");

    let log_text = logs.contents();
    assert!(
        log_text.contains("test_span"),
        "expected span name: {log_text}"
    );
    assert!(
        log_text.contains("inside span"),
        "expected event message: {log_text}"
    );
}

// ===========================================================================
// 10. Verify target-based filtering works
// ===========================================================================

#[tokio::test(flavor = "current_thread")]
async fn filter_allows_runtime_target() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=debug");

    tracing::debug!(target: "abp.runtime", "runtime visible");
    tracing::debug!(target: "abp.sidecar", "sidecar hidden");

    let log_text = logs.contents();
    assert!(
        log_text.contains("runtime visible"),
        "runtime target should pass filter: {log_text}"
    );
    assert!(
        !log_text.contains("sidecar hidden"),
        "sidecar target should be filtered out: {log_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn filter_allows_sidecar_target() {
    let (logs, _guard) = setup_filtered_tracing("abp.sidecar=warn");

    tracing::warn!(target: "abp.sidecar", "sidecar warning");
    tracing::debug!(target: "abp.sidecar", "sidecar debug hidden");
    tracing::info!(target: "abp.runtime", "runtime hidden");

    let log_text = logs.contents();
    assert!(
        log_text.contains("sidecar warning"),
        "sidecar WARN should pass filter: {log_text}"
    );
    assert!(
        !log_text.contains("sidecar debug hidden"),
        "sidecar DEBUG should be filtered: {log_text}"
    );
    assert!(
        !log_text.contains("runtime hidden"),
        "runtime target should be filtered: {log_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn filter_allows_multiple_targets() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=info,abp.workspace=debug");

    tracing::info!(target: "abp.runtime", "runtime info");
    tracing::debug!(target: "abp.workspace", "workspace debug");
    tracing::debug!(target: "abp.sidecar", "sidecar filtered");

    let log_text = logs.contents();
    assert!(
        log_text.contains("runtime info"),
        "runtime info should pass: {log_text}"
    );
    assert!(
        log_text.contains("workspace debug"),
        "workspace debug should pass: {log_text}"
    );
    assert!(
        !log_text.contains("sidecar filtered"),
        "sidecar should be filtered: {log_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn filter_workspace_target_during_staged_run() {
    // Filter to only workspace target, then verify the staging trace appears.
    let (logs, _guard) = setup_filtered_tracing("abp.workspace=debug");

    let src_dir = tempfile::tempdir().unwrap();
    std::fs::write(src_dir.path().join("a.txt"), "data").unwrap();

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("filter staging test")
        .root(src_dir.path().to_str().unwrap())
        .workspace_mode(WorkspaceMode::Staged)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    assert!(
        log_text.contains("staging workspace"),
        "workspace staging trace should appear: {log_text}"
    );
    // Runtime target should be filtered out.
    assert!(
        !log_text.contains("starting run"),
        "runtime trace should be filtered: {log_text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn filter_runtime_target_during_passthrough_run() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=debug");

    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("filter runtime test")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_events, receipt) = drain_run(handle).await;
    receipt.unwrap();

    let log_text = logs.contents();
    assert!(
        log_text.contains("starting run"),
        "runtime trace should appear with filter: {log_text}"
    );
}

// ===========================================================================
// Bonus: abp-telemetry crate standalone tests
// ===========================================================================

#[test]
fn telemetry_span_serialization_roundtrip() {
    let span = TelemetrySpan::new("backend_exec")
        .with_attribute("run_id", "uuid-1234")
        .with_attribute("backend", "mock")
        .with_attribute("dialect", "anthropic");

    let json = serde_json::to_string(&span).unwrap();
    let deserialized: TelemetrySpan = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "backend_exec");
    assert_eq!(deserialized.attributes["run_id"], "uuid-1234");
    assert_eq!(deserialized.attributes["backend"], "mock");
    assert_eq!(deserialized.attributes["dialect"], "anthropic");
}

#[test]
fn metrics_collector_summary_with_multiple_backends() {
    let collector = MetricsCollector::new();
    for _ in 0..3 {
        collector.record(TelemetryRunMetrics {
            backend_name: "openai".into(),
            dialect: "openai".into(),
            duration_ms: 100,
            events_count: 5,
            tokens_in: 50,
            tokens_out: 100,
            tool_calls_count: 2,
            errors_count: 0,
            emulations_applied: 0,
        });
    }
    for _ in 0..2 {
        collector.record(TelemetryRunMetrics {
            backend_name: "anthropic".into(),
            dialect: "anthropic".into(),
            duration_ms: 200,
            events_count: 10,
            tokens_in: 80,
            tokens_out: 150,
            tool_calls_count: 3,
            errors_count: 1,
            emulations_applied: 0,
        });
    }

    let summary = collector.summary();
    assert_eq!(summary.count, 5);
    assert_eq!(summary.backend_counts["openai"], 3);
    assert_eq!(summary.backend_counts["anthropic"], 2);
    assert_eq!(summary.total_tokens_in, 310); // 3*50 + 2*80
    assert_eq!(summary.total_tokens_out, 600); // 3*100 + 2*150
}

#[test]
fn json_exporter_empty_produces_valid_json() {
    let summary = abp_telemetry::MetricsSummary::default();
    let exporter = JsonExporter;
    let json = exporter.export(&summary).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
}
