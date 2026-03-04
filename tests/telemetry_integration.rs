#![allow(clippy::all)]
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

// ===========================================================================
// 11. RunMetrics construction and defaults
// ===========================================================================

#[test]
fn run_metrics_default_is_zeroed() {
    let m = TelemetryRunMetrics::default();
    assert_eq!(m.backend_name, "");
    assert_eq!(m.dialect, "");
    assert_eq!(m.duration_ms, 0);
    assert_eq!(m.events_count, 0);
    assert_eq!(m.tokens_in, 0);
    assert_eq!(m.tokens_out, 0);
    assert_eq!(m.tool_calls_count, 0);
    assert_eq!(m.errors_count, 0);
    assert_eq!(m.emulations_applied, 0);
}

#[test]
fn run_metrics_clone_equals_original() {
    let m = TelemetryRunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 123,
        events_count: 10,
        tokens_in: 500,
        tokens_out: 1000,
        tool_calls_count: 5,
        errors_count: 2,
        emulations_applied: 1,
    };
    let cloned = m.clone();
    assert_eq!(m, cloned);
}

#[test]
fn run_metrics_debug_format_contains_fields() {
    let m = TelemetryRunMetrics {
        backend_name: "sidecar".into(),
        dialect: "anthropic".into(),
        duration_ms: 42,
        ..Default::default()
    };
    let dbg = format!("{m:?}");
    assert!(dbg.contains("sidecar"));
    assert!(dbg.contains("anthropic"));
    assert!(dbg.contains("42"));
}

#[test]
fn run_metrics_partial_eq_different_fields() {
    let a = TelemetryRunMetrics {
        backend_name: "a".into(),
        ..Default::default()
    };
    let b = TelemetryRunMetrics {
        backend_name: "b".into(),
        ..Default::default()
    };
    assert_ne!(a, b);
}

#[test]
fn run_metrics_serde_all_fields_present() {
    let m = TelemetryRunMetrics {
        backend_name: "test".into(),
        dialect: "gemini".into(),
        duration_ms: 999,
        events_count: 50,
        tokens_in: 200,
        tokens_out: 400,
        tool_calls_count: 10,
        errors_count: 3,
        emulations_applied: 2,
    };
    let val: serde_json::Value = serde_json::to_value(&m).unwrap();
    assert_eq!(val["backend_name"], "test");
    assert_eq!(val["dialect"], "gemini");
    assert_eq!(val["duration_ms"], 999);
    assert_eq!(val["events_count"], 50);
    assert_eq!(val["tokens_in"], 200);
    assert_eq!(val["tokens_out"], 400);
    assert_eq!(val["tool_calls_count"], 10);
    assert_eq!(val["errors_count"], 3);
    assert_eq!(val["emulations_applied"], 2);
}

// ===========================================================================
// 12. MetricsCollector lifecycle
// ===========================================================================

#[test]
fn collector_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_record_increments_len() {
    let c = MetricsCollector::new();
    for i in 1..=5 {
        c.record(TelemetryRunMetrics {
            backend_name: format!("b{i}"),
            ..Default::default()
        });
        assert_eq!(c.len(), i);
    }
}

#[test]
fn collector_clear_resets_to_empty() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics::default());
    c.record(TelemetryRunMetrics::default());
    assert_eq!(c.len(), 2);
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    // Summary after clear is default.
    let s = c.summary();
    assert_eq!(s.count, 0);
}

#[test]
fn collector_runs_preserves_insertion_order() {
    let c = MetricsCollector::new();
    for name in ["alpha", "beta", "gamma"] {
        c.record(TelemetryRunMetrics {
            backend_name: name.into(),
            ..Default::default()
        });
    }
    let runs = c.runs();
    assert_eq!(runs[0].backend_name, "alpha");
    assert_eq!(runs[1].backend_name, "beta");
    assert_eq!(runs[2].backend_name, "gamma");
}

#[test]
fn collector_runs_returns_independent_snapshot() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics::default());
    let snap1 = c.runs();
    c.record(TelemetryRunMetrics::default());
    let snap2 = c.runs();
    assert_eq!(snap1.len(), 1);
    assert_eq!(snap2.len(), 2);
}

#[test]
fn collector_clone_shares_state() {
    let c1 = MetricsCollector::new();
    let c2 = c1.clone();
    c1.record(TelemetryRunMetrics::default());
    assert_eq!(c2.len(), 1);
}

#[test]
fn collector_record_after_clear() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics::default());
    c.clear();
    c.record(TelemetryRunMetrics {
        backend_name: "after-clear".into(),
        ..Default::default()
    });
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "after-clear");
}

// ===========================================================================
// 13. MetricsSummary aggregation math
// ===========================================================================

#[test]
fn summary_mean_duration_single_run() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: 100,
        ..Default::default()
    });
    let s = c.summary();
    assert!((s.mean_duration_ms - 100.0).abs() < f64::EPSILON);
}

#[test]
fn summary_mean_duration_multiple_runs() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(TelemetryRunMetrics {
            duration_ms: d,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_two_elements() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: 10,
        ..Default::default()
    });
    c.record(TelemetryRunMetrics {
        duration_ms: 20,
        ..Default::default()
    });
    let s = c.summary();
    assert!((s.p50_duration_ms - 15.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p99_two_elements() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: 100,
        ..Default::default()
    });
    c.record(TelemetryRunMetrics {
        duration_ms: 200,
        ..Default::default()
    });
    let s = c.summary();
    // p99 on [100,200]: rank = 0.99 * 1 = 0.99 → lerp(100,200,0.99) = 199.0
    assert!((s.p99_duration_ms - 199.0).abs() < f64::EPSILON);
}

#[test]
fn summary_total_tokens_accumulate() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(TelemetryRunMetrics {
            tokens_in: 10,
            tokens_out: 20,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 50);
    assert_eq!(s.total_tokens_out, 100);
}

#[test]
fn summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    for _ in 0..3 {
        c.record(TelemetryRunMetrics {
            errors_count: 0,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    for _ in 0..4 {
        c.record(TelemetryRunMetrics {
            errors_count: 1,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_mixed() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        errors_count: 0,
        ..Default::default()
    });
    c.record(TelemetryRunMetrics {
        errors_count: 3,
        ..Default::default()
    });
    let s = c.summary();
    // 3 errors / 2 runs = 1.5
    assert!((s.error_rate - 1.5).abs() < f64::EPSILON);
}

#[test]
fn summary_backend_counts_deterministic_order() {
    let c = MetricsCollector::new();
    for name in ["zebra", "alpha", "middle"] {
        c.record(TelemetryRunMetrics {
            backend_name: name.into(),
            ..Default::default()
        });
    }
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn summary_p50_identical_durations() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(TelemetryRunMetrics {
            duration_ms: 42,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_large_dataset_p50_p99() {
    let c = MetricsCollector::new();
    for d in 1..=1000 {
        c.record(TelemetryRunMetrics {
            duration_ms: d,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    // mean of 1..=1000 is 500.5
    assert!((s.mean_duration_ms - 500.5).abs() < f64::EPSILON);
    // p50 of 1..=1000 should be ~500.5
    assert!((s.p50_duration_ms - 500.5).abs() < 1.0);
    // p99 should be close to 990
    assert!(s.p99_duration_ms > 989.0);
}

// ===========================================================================
// 14. MetricsSummary default and serde
// ===========================================================================

#[test]
fn metrics_summary_default_all_zero() {
    let s = abp_telemetry::MetricsSummary::default();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.p50_duration_ms, 0.0);
    assert_eq!(s.p99_duration_ms, 0.0);
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
    assert_eq!(s.error_rate, 0.0);
    assert!(s.backend_counts.is_empty());
}

#[test]
fn metrics_summary_serde_roundtrip_with_backends() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "openai".into(),
        duration_ms: 100,
        tokens_in: 50,
        tokens_out: 100,
        errors_count: 1,
        ..Default::default()
    });
    c.record(TelemetryRunMetrics {
        backend_name: "anthropic".into(),
        duration_ms: 200,
        tokens_in: 80,
        tokens_out: 160,
        errors_count: 0,
        ..Default::default()
    });
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: abp_telemetry::MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_clone_eq() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        duration_ms: 50,
        ..Default::default()
    });
    let s = c.summary();
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_debug_format() {
    let s = abp_telemetry::MetricsSummary::default();
    let dbg = format!("{s:?}");
    assert!(dbg.contains("MetricsSummary"));
    assert!(dbg.contains("count"));
}

// ===========================================================================
// 15. TelemetrySpan creation and attributes
// ===========================================================================

#[test]
fn span_new_empty_attributes() {
    let span = TelemetrySpan::new("operation");
    assert_eq!(span.name, "operation");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_with_attribute_chainable() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
}

#[test]
fn span_attribute_overwrites_duplicate_key() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "first")
        .with_attribute("key", "second");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "second");
}

#[test]
fn span_accepts_string_types() {
    let name = String::from("dynamic");
    let key = String::from("k");
    let val = String::from("v");
    let span = TelemetrySpan::new(name).with_attribute(key, val);
    assert_eq!(span.name, "dynamic");
    assert_eq!(span.attributes["k"], "v");
}

#[test]
fn span_attributes_btreemap_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z_key", "last")
        .with_attribute("a_key", "first")
        .with_attribute("m_key", "middle");
    let keys: Vec<&String> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
}

#[test]
fn span_serde_preserves_attribute_order() {
    let span = TelemetrySpan::new("ordered")
        .with_attribute("z", "3")
        .with_attribute("a", "1")
        .with_attribute("m", "2");
    let json = serde_json::to_string(&span).unwrap();
    // BTreeMap ensures "a" < "m" < "z" in JSON output.
    let a_pos = json.find("\"a\"").unwrap();
    let m_pos = json.find("\"m\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn span_clone_independent() {
    let span = TelemetrySpan::new("op").with_attribute("k", "v");
    let cloned = span.clone();
    assert_eq!(span.name, cloned.name);
    assert_eq!(span.attributes, cloned.attributes);
}

#[test]
fn span_emit_no_panic_without_subscriber() {
    // No subscriber set — emit should not panic.
    let span = TelemetrySpan::new("safe").with_attribute("test", "true");
    span.emit();
}

#[test]
fn span_debug_format() {
    let span = TelemetrySpan::new("debug_test").with_attribute("key", "val");
    let dbg = format!("{span:?}");
    assert!(dbg.contains("debug_test"));
    assert!(dbg.contains("key"));
}

// ===========================================================================
// 16. JsonExporter scenarios
// ===========================================================================

#[test]
fn json_exporter_default_trait() {
    let exporter = JsonExporter;
    let s = abp_telemetry::MetricsSummary::default();
    let json = exporter.export(&s).unwrap();
    assert!(json.contains("\"count\": 0"));
}

#[test]
fn json_exporter_pretty_print_multiline() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "mock".into(),
        duration_ms: 100,
        ..Default::default()
    });
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    // Pretty print should have newlines.
    assert!(json.contains('\n'));
}

#[test]
fn json_exporter_output_deserializes_to_summary() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "test".into(),
        duration_ms: 50,
        tokens_in: 100,
        tokens_out: 200,
        errors_count: 1,
        ..Default::default()
    });
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let s2: abp_telemetry::MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn json_exporter_multiple_backends_all_present() {
    let c = MetricsCollector::new();
    for name in ["openai", "anthropic", "gemini", "mock"] {
        c.record(TelemetryRunMetrics {
            backend_name: name.into(),
            ..Default::default()
        });
    }
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    for name in ["openai", "anthropic", "gemini", "mock"] {
        assert!(json.contains(name), "missing backend {name} in JSON");
    }
}

// ===========================================================================
// 17. Custom exporter implementations
// ===========================================================================

/// A custom exporter that produces CSV-like output.
struct CsvExporter;

impl TelemetryExporter for CsvExporter {
    fn export(&self, summary: &abp_telemetry::MetricsSummary) -> Result<String, String> {
        Ok(format!(
            "count,mean_duration_ms,error_rate\n{},{:.1},{}",
            summary.count, summary.mean_duration_ms, summary.error_rate
        ))
    }
}

#[test]
fn custom_exporter_csv_format() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: 100,
        errors_count: 1,
        ..Default::default()
    });
    let s = c.summary();
    let exporter = CsvExporter;
    let csv = exporter.export(&s).unwrap();
    assert!(csv.starts_with("count,mean_duration_ms,error_rate\n"));
    assert!(csv.contains("1,100.0,1"));
}

/// An exporter that always fails, for negative-path testing.
struct FailingExporter;

impl TelemetryExporter for FailingExporter {
    fn export(&self, _: &abp_telemetry::MetricsSummary) -> Result<String, String> {
        Err("export failed".into())
    }
}

#[test]
fn failing_exporter_returns_error() {
    let s = abp_telemetry::MetricsSummary::default();
    let exporter = FailingExporter;
    let result = exporter.export(&s);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "export failed");
}

#[test]
fn exporter_trait_object_dispatch() {
    let exporters: Vec<Box<dyn TelemetryExporter>> =
        vec![Box::new(JsonExporter), Box::new(CsvExporter)];
    let s = abp_telemetry::MetricsSummary::default();
    for e in &exporters {
        let result = e.export(&s);
        assert!(result.is_ok());
    }
}

// ===========================================================================
// 18. Multiple concurrent collectors
// ===========================================================================

#[test]
fn independent_collectors_do_not_share_state() {
    let c1 = MetricsCollector::new();
    let c2 = MetricsCollector::new();
    c1.record(TelemetryRunMetrics::default());
    assert_eq!(c1.len(), 1);
    assert_eq!(c2.len(), 0);
}

#[test]
fn concurrent_record_and_summary_stress() {
    let c = MetricsCollector::new();
    let mut handles = Vec::new();
    for i in 0..50u64 {
        let cc = c.clone();
        handles.push(std::thread::spawn(move || {
            cc.record(TelemetryRunMetrics {
                backend_name: format!("b{}", i % 5),
                duration_ms: i * 10,
                tokens_in: i,
                tokens_out: i * 2,
                errors_count: if i % 7 == 0 { 1 } else { 0 },
                ..Default::default()
            });
            let _ = cc.summary();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 50);
    let s = c.summary();
    assert_eq!(s.count, 50);
    assert_eq!(s.backend_counts.values().sum::<usize>(), 50);
}

#[test]
fn concurrent_clear_during_record() {
    let c = MetricsCollector::new();
    let c1 = c.clone();
    let c2 = c.clone();
    let h1 = std::thread::spawn(move || {
        for _ in 0..100 {
            c1.record(TelemetryRunMetrics::default());
        }
    });
    let h2 = std::thread::spawn(move || {
        for _ in 0..10 {
            c2.clear();
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    // No panic — final len may vary due to interleaving.
    let _ = c.len();
}

// ===========================================================================
// 19. Edge cases
// ===========================================================================

#[test]
fn collector_summary_single_zero_duration() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: 0,
        ..Default::default()
    });
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.p50_duration_ms, 0.0);
    assert_eq!(s.p99_duration_ms, 0.0);
}

#[test]
fn collector_summary_max_u64_duration() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        duration_ms: u64::MAX,
        ..Default::default()
    });
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, u64::MAX as f64);
}

#[test]
fn run_metrics_empty_strings() {
    let m = TelemetryRunMetrics {
        backend_name: "".into(),
        dialect: "".into(),
        ..Default::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: TelemetryRunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn span_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_unicode_name_and_attributes() {
    let span = TelemetrySpan::new("日本語テスト").with_attribute("キー", "値");
    assert_eq!(span.name, "日本語テスト");
    assert_eq!(span.attributes["キー"], "値");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "日本語テスト");
}

#[test]
fn span_many_attributes() {
    let mut span = TelemetrySpan::new("big");
    for i in 0..100 {
        span = span.with_attribute(format!("key_{i}"), format!("val_{i}"));
    }
    assert_eq!(span.attributes.len(), 100);
}

// ===========================================================================
// 20. Serde roundtrip exhaustive
// ===========================================================================

#[test]
fn run_metrics_json_roundtrip_with_high_values() {
    let m = TelemetryRunMetrics {
        backend_name: "stress".into(),
        dialect: "custom".into(),
        duration_ms: 999_999,
        events_count: 100_000,
        tokens_in: 1_000_000,
        tokens_out: 2_000_000,
        tool_calls_count: 50_000,
        errors_count: 10_000,
        emulations_applied: 500,
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: TelemetryRunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn span_json_roundtrip_empty_attributes() {
    let span = TelemetrySpan::new("minimal");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "minimal");
    assert!(span2.attributes.is_empty());
}

#[test]
fn metrics_summary_json_field_names() {
    let s = abp_telemetry::MetricsSummary::default();
    let val: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert!(val.get("count").is_some());
    assert!(val.get("mean_duration_ms").is_some());
    assert!(val.get("p50_duration_ms").is_some());
    assert!(val.get("p99_duration_ms").is_some());
    assert!(val.get("total_tokens_in").is_some());
    assert!(val.get("total_tokens_out").is_some());
    assert!(val.get("error_rate").is_some());
    assert!(val.get("backend_counts").is_some());
}

// ===========================================================================
// 21. Telemetry hooks — span emit with subscriber
// ===========================================================================

#[test]
fn span_emit_includes_attributes_in_log() {
    let (logs, _guard) = setup_tracing();
    let span = TelemetrySpan::new("hook_test")
        .with_attribute("run_id", "r-001")
        .with_attribute("backend", "sidecar");
    span.emit();
    let text = logs.contents();
    assert!(text.contains("hook_test"), "span name missing: {text}");
}

#[test]
fn multiple_span_emits_all_captured() {
    let (logs, _guard) = setup_tracing();
    for i in 0..5 {
        TelemetrySpan::new(format!("span_{i}")).emit();
    }
    let text = logs.contents();
    for i in 0..5 {
        assert!(
            text.contains(&format!("span_{i}")),
            "missing span_{i}: {text}"
        );
    }
}

// ===========================================================================
// 22. Metric aggregation across backends
// ===========================================================================

#[test]
fn aggregation_single_backend_repeated() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(TelemetryRunMetrics {
            backend_name: "only".into(),
            duration_ms: 50,
            tokens_in: 10,
            tokens_out: 20,
            errors_count: 0,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["only"], 10);
    assert_eq!(s.total_tokens_in, 100);
    assert_eq!(s.total_tokens_out, 200);
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn aggregation_many_backends() {
    let c = MetricsCollector::new();
    let backends = ["openai", "anthropic", "gemini", "mock", "sidecar"];
    for (i, name) in backends.iter().enumerate() {
        c.record(TelemetryRunMetrics {
            backend_name: (*name).into(),
            duration_ms: (i as u64 + 1) * 100,
            tokens_in: (i as u64 + 1) * 10,
            tokens_out: (i as u64 + 1) * 20,
            ..Default::default()
        });
    }
    let s = c.summary();
    assert_eq!(s.count, 5);
    assert_eq!(s.backend_counts.len(), 5);
    for name in &backends {
        assert!(s.backend_counts.contains_key(*name));
    }
}

// ===========================================================================
// 23. Resource identification
// ===========================================================================

#[test]
fn run_metrics_backend_name_identifies_resource() {
    let m = TelemetryRunMetrics {
        backend_name: "sidecar:claude".into(),
        dialect: "anthropic".into(),
        ..Default::default()
    };
    assert!(m.backend_name.starts_with("sidecar:"));
    assert_eq!(m.dialect, "anthropic");
}

#[test]
fn summary_backend_counts_serve_as_resource_inventory() {
    let c = MetricsCollector::new();
    for _ in 0..3 {
        c.record(TelemetryRunMetrics {
            backend_name: "sidecar:node".into(),
            ..Default::default()
        });
    }
    for _ in 0..2 {
        c.record(TelemetryRunMetrics {
            backend_name: "sidecar:python".into(),
            ..Default::default()
        });
    }
    let s = c.summary();
    assert_eq!(s.backend_counts["sidecar:node"], 3);
    assert_eq!(s.backend_counts["sidecar:python"], 2);
}

// ===========================================================================
// 24. JSON structured logging — additional coverage
// ===========================================================================

#[test]
fn json_exporter_debug_format() {
    let exporter = JsonExporter;
    let dbg = format!("{exporter:?}");
    assert!(dbg.contains("JsonExporter"));
}

#[test]
fn json_export_contains_all_summary_fields() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "check".into(),
        duration_ms: 10,
        tokens_in: 5,
        tokens_out: 15,
        errors_count: 1,
        ..Default::default()
    });
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    for field in [
        "count",
        "mean_duration_ms",
        "p50_duration_ms",
        "p99_duration_ms",
        "total_tokens_in",
        "total_tokens_out",
        "error_rate",
        "backend_counts",
    ] {
        assert!(json.contains(field), "missing field {field} in JSON");
    }
}

// ===========================================================================
// 25. Integration: collector → summary → export pipeline
// ===========================================================================

#[test]
fn full_pipeline_record_summarize_export() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "pipeline".into(),
        dialect: "openai".into(),
        duration_ms: 250,
        events_count: 15,
        tokens_in: 300,
        tokens_out: 600,
        tool_calls_count: 8,
        errors_count: 0,
        emulations_applied: 2,
    });
    c.record(TelemetryRunMetrics {
        backend_name: "pipeline".into(),
        dialect: "openai".into(),
        duration_ms: 350,
        events_count: 20,
        tokens_in: 400,
        tokens_out: 800,
        tool_calls_count: 12,
        errors_count: 1,
        emulations_applied: 0,
    });

    let s = c.summary();
    assert_eq!(s.count, 2);
    assert!((s.mean_duration_ms - 300.0).abs() < f64::EPSILON);
    assert_eq!(s.total_tokens_in, 700);
    assert_eq!(s.total_tokens_out, 1400);
    assert!((s.error_rate - 0.5).abs() < f64::EPSILON);
    assert_eq!(s.backend_counts["pipeline"], 2);

    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 2);
    assert_eq!(parsed["total_tokens_in"], 700);
}

#[test]
fn pipeline_clear_and_re_record() {
    let c = MetricsCollector::new();
    c.record(TelemetryRunMetrics {
        backend_name: "first".into(),
        duration_ms: 100,
        ..Default::default()
    });
    let s1 = c.summary();
    assert_eq!(s1.count, 1);

    c.clear();
    c.record(TelemetryRunMetrics {
        backend_name: "second".into(),
        duration_ms: 200,
        ..Default::default()
    });
    let s2 = c.summary();
    assert_eq!(s2.count, 1);
    assert!((s2.mean_duration_ms - 200.0).abs() < f64::EPSILON);
    assert!(s2.backend_counts.contains_key("second"));
    assert!(!s2.backend_counts.contains_key("first"));
}
