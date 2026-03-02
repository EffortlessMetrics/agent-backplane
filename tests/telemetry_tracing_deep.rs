// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for `abp-telemetry`: span lifecycle, metric counters/histograms,
//! serde roundtrips, tracing subscriber integration, concurrent operations,
//! custom tracing targets, error span recording, and metric aggregation.

use std::sync::{Arc, Mutex};
use std::thread;

use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};

// ---------------------------------------------------------------------------
// Log-capture infrastructure (reusable across tracing tests)
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

/// Install a JSON-formatted tracing subscriber.
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

// ---------------------------------------------------------------------------
// Helper factories
// ---------------------------------------------------------------------------

fn sample(backend: &str, duration: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test".to_string(),
        duration_ms: duration,
        events_count: 5,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 3,
        errors_count: errors,
        emulations_applied: 1,
    }
}

#[allow(clippy::too_many_arguments)]
fn custom_metrics(
    backend: &str,
    duration: u64,
    tokens_in: u64,
    tokens_out: u64,
    events: u64,
    tools: u64,
    errors: u64,
    emulations: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "custom".to_string(),
        duration_ms: duration,
        events_count: events,
        tokens_in,
        tokens_out,
        tool_calls_count: tools,
        errors_count: errors,
        emulations_applied: emulations,
    }
}

// ===========================================================================
// 1. Span creation and lifecycle
// ===========================================================================

#[test]
fn span_creation_basic() {
    let span = TelemetrySpan::new("test-op");
    assert_eq!(span.name, "test-op");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_creation_with_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_creation_from_string_type() {
    let name = String::from("dynamic-name");
    let span = TelemetrySpan::new(name);
    assert_eq!(span.name, "dynamic-name");
}

#[test]
fn span_clone_is_independent() {
    let span = TelemetrySpan::new("original").with_attribute("k", "v");
    let mut cloned = span.clone();
    cloned.attributes.insert("extra".into(), "val".into());
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(cloned.attributes.len(), 2);
}

// ===========================================================================
// 2. Nested span relationships (parent-child via tracing)
// ===========================================================================

#[test]
fn nested_tracing_spans_captured() {
    let (logs, _guard) = setup_tracing();

    let _outer = tracing::info_span!("parent_op").entered();
    tracing::info!("inside parent");
    {
        let _inner = tracing::info_span!("child_op").entered();
        tracing::info!("inside child");
    }

    assert!(logs.contains("parent_op"), "logs: {}", logs.contents());
    assert!(logs.contains("child_op"), "logs: {}", logs.contents());
    assert!(logs.contains("inside parent"), "logs: {}", logs.contents());
    assert!(logs.contains("inside child"), "logs: {}", logs.contents());
}

#[test]
fn deeply_nested_spans() {
    let (logs, _guard) = setup_tracing();

    let _l1 = tracing::info_span!("level1").entered();
    let _l2 = tracing::info_span!("level2").entered();
    let _l3 = tracing::info_span!("level3").entered();
    tracing::info!("deep message");

    assert!(logs.contains("level1"), "logs: {}", logs.contents());
    assert!(logs.contains("level2"), "logs: {}", logs.contents());
    assert!(logs.contains("level3"), "logs: {}", logs.contents());
    assert!(logs.contains("deep message"), "logs: {}", logs.contents());
}

// ===========================================================================
// 3. Span attributes/fields
// ===========================================================================

#[test]
fn span_single_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("backend", "mock");
    assert_eq!(span.attributes["backend"], "mock");
}

#[test]
fn span_multiple_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
}

#[test]
fn span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "old")
        .with_attribute("key", "new");
    assert_eq!(span.attributes["key"], "new");
    assert_eq!(span.attributes.len(), 1);
}

#[test]
fn span_attributes_deterministic_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("zebra", "z")
        .with_attribute("alpha", "a")
        .with_attribute("middle", "m");
    let keys: Vec<_> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn span_attribute_empty_key_and_value() {
    let span = TelemetrySpan::new("op").with_attribute("", "");
    assert_eq!(span.attributes[""], "");
}

// ===========================================================================
// 4. Event recording within spans
// ===========================================================================

#[test]
fn event_recorded_inside_span() {
    let (logs, _guard) = setup_tracing();

    let _span = tracing::info_span!("work_span").entered();
    tracing::info!(event_type = "tool_call", tool = "read_file", "tool invoked");

    assert!(logs.contains("tool invoked"), "logs: {}", logs.contents());
    assert!(logs.contains("work_span"), "logs: {}", logs.contents());
}

#[test]
fn multiple_events_within_single_span() {
    let (logs, _guard) = setup_tracing();

    let _span = tracing::info_span!("multi_event").entered();
    tracing::info!("event_one");
    tracing::debug!("event_two");
    tracing::warn!("event_three");

    assert!(logs.contains("event_one"), "logs: {}", logs.contents());
    assert!(logs.contains("event_two"), "logs: {}", logs.contents());
    assert!(logs.contains("event_three"), "logs: {}", logs.contents());
}

#[test]
fn telemetry_span_emit_produces_output() {
    let (logs, _guard) = setup_tracing();

    let span = TelemetrySpan::new("emit_test").with_attribute("backend", "mock");
    span.emit();

    assert!(logs.contains("telemetry_span"), "logs: {}", logs.contents());
    assert!(logs.contains("emit_test"), "logs: {}", logs.contents());
}

// ===========================================================================
// 5. Counter metrics (increment, get value)
// ===========================================================================

#[test]
fn collector_increments_run_count() {
    let c = MetricsCollector::new();
    for i in 0..5 {
        c.record(sample("mock", i * 10, 0));
    }
    assert_eq!(c.len(), 5);
}

#[test]
fn collector_tracks_error_counts() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 2));
    c.record(sample("b", 20, 3));
    let runs = c.runs();
    let total_errors: u64 = runs.iter().map(|r| r.errors_count).sum();
    assert_eq!(total_errors, 5);
}

#[test]
fn collector_tracks_tool_call_counts() {
    let c = MetricsCollector::new();
    c.record(custom_metrics("a", 10, 0, 0, 0, 7, 0, 0));
    c.record(custom_metrics("b", 20, 0, 0, 0, 3, 0, 0));
    let runs = c.runs();
    let total_tools: u64 = runs.iter().map(|r| r.tool_calls_count).sum();
    assert_eq!(total_tools, 10);
}

#[test]
fn collector_tracks_event_counts() {
    let c = MetricsCollector::new();
    c.record(custom_metrics("a", 10, 0, 0, 42, 0, 0, 0));
    let runs = c.runs();
    assert_eq!(runs[0].events_count, 42);
}

// ===========================================================================
// 6. Histogram metrics (record values, get stats)
// ===========================================================================

#[test]
fn summary_duration_histogram_mean() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_duration_histogram_p50_two_values() {
    let c = MetricsCollector::new();
    c.record(sample("a", 100, 0));
    c.record(sample("a", 200, 0));
    let s = c.summary();
    assert!((s.p50_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_duration_histogram_p99_large_set() {
    let c = MetricsCollector::new();
    for d in 1..=1000 {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 989.0);
    assert!(s.p99_duration_ms <= 1000.0);
}

#[test]
fn summary_duration_histogram_single_value() {
    let c = MetricsCollector::new();
    c.record(sample("a", 42, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_duration_histogram_identical_values() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(sample("a", 50, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 50.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 7. Tracing subscriber integration
// ===========================================================================

#[test]
fn subscriber_captures_info_level() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("hello from info");
    assert!(
        logs.contains("hello from info"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn subscriber_captures_debug_level() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!("hello from debug");
    assert!(
        logs.contains("hello from debug"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn subscriber_captures_trace_level() {
    let (logs, _guard) = setup_tracing();
    tracing::trace!("hello from trace");
    assert!(
        logs.contains("hello from trace"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn subscriber_captures_warn_level() {
    let (logs, _guard) = setup_tracing();
    tracing::warn!("hello from warn");
    assert!(
        logs.contains("hello from warn"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn subscriber_captures_error_level() {
    let (logs, _guard) = setup_tracing();
    tracing::error!("hello from error");
    assert!(
        logs.contains("hello from error"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn json_subscriber_produces_valid_json() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(key = "value", "json_test");
    let content = logs.contents();
    // Each line of JSON output should parse
    for line in content.lines().filter(|l| !l.is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parsed.is_object());
    }
}

// ===========================================================================
// 8. Structured logging with tracing targets
// ===========================================================================

#[test]
fn structured_fields_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(
        backend = "mock",
        run_id = "abc-123",
        events = 5,
        "structured event"
    );
    assert!(
        logs.contains("structured event"),
        "logs: {}",
        logs.contents()
    );
    assert!(logs.contains("mock"), "logs: {}", logs.contents());
}

#[test]
fn structured_fields_in_json_mode() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(
        backend = "openai",
        duration_ms = 42,
        "structured json event"
    );
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["fields"]["backend"], "openai");
        assert_eq!(parsed["fields"]["duration_ms"], 42);
    }
}

// ===========================================================================
// 9. Span timing (duration tracking via metrics)
// ===========================================================================

#[test]
fn duration_tracking_zero() {
    let c = MetricsCollector::new();
    c.record(sample("a", 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 0.0).abs() < f64::EPSILON);
}

#[test]
fn duration_tracking_large_values() {
    let c = MetricsCollector::new();
    c.record(sample("a", u64::MAX / 2, 0));
    let runs = c.runs();
    assert_eq!(runs[0].duration_ms, u64::MAX / 2);
}

#[test]
fn duration_ordering_preserved() {
    let c = MetricsCollector::new();
    c.record(sample("a", 300, 0));
    c.record(sample("a", 100, 0));
    c.record(sample("a", 200, 0));
    let runs = c.runs();
    assert_eq!(runs[0].duration_ms, 300);
    assert_eq!(runs[1].duration_ms, 100);
    assert_eq!(runs[2].duration_ms, 200);
}

// ===========================================================================
// 10. Concurrent span operations
// ===========================================================================

#[test]
fn concurrent_collector_recording() {
    let c = MetricsCollector::new();
    let mut handles = Vec::new();
    for i in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample("concurrent", i * 5, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
}

#[test]
fn concurrent_summary_reads() {
    let c = MetricsCollector::new();
    for i in 0..10 {
        c.record(sample("pre", i * 10, 0));
    }
    let mut handles = Vec::new();
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || cc.summary()));
    }
    let summaries: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for s in &summaries {
        assert_eq!(s.count, 10);
    }
}

#[test]
fn concurrent_read_write_mix() {
    let c = MetricsCollector::new();
    c.record(sample("initial", 50, 0));

    let mut handles = Vec::new();
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            if i % 2 == 0 {
                cc.record(sample("writer", i * 10, 0));
            } else {
                let _ = cc.summary();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // 1 initial + 5 even-numbered writer threads
    assert_eq!(c.len(), 6);
}

#[test]
fn concurrent_clear_and_record() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(sample("pre", 10, 0));
    }

    let c1 = c.clone();
    let t1 = thread::spawn(move || {
        c1.clear();
    });
    let c2 = c.clone();
    let t2 = thread::spawn(move || {
        c2.record(sample("post", 20, 0));
    });
    t1.join().unwrap();
    t2.join().unwrap();
    // After clear + one record, len is 0 or 1 depending on order
    assert!(c.len() <= 1);
}

// ===========================================================================
// 11. Metric aggregation across multiple spans
// ===========================================================================

#[test]
fn aggregation_total_tokens_multiple_backends() {
    let c = MetricsCollector::new();
    c.record(custom_metrics("openai", 10, 500, 1000, 0, 0, 0, 0));
    c.record(custom_metrics("claude", 20, 300, 600, 0, 0, 0, 0));
    c.record(custom_metrics("gemini", 30, 200, 400, 0, 0, 0, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 1000);
    assert_eq!(s.total_tokens_out, 2000);
}

#[test]
fn aggregation_backend_counts_accurate() {
    let c = MetricsCollector::new();
    c.record(sample("openai", 10, 0));
    c.record(sample("openai", 20, 0));
    c.record(sample("claude", 30, 0));
    c.record(sample("gemini", 40, 0));
    c.record(sample("gemini", 50, 0));
    c.record(sample("gemini", 60, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["openai"], 2);
    assert_eq!(s.backend_counts["claude"], 1);
    assert_eq!(s.backend_counts["gemini"], 3);
    assert_eq!(s.backend_counts.len(), 3);
}

#[test]
fn aggregation_error_rate_all_errors() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(sample("a", 10, 1));
    }
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn aggregation_error_rate_no_errors() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(sample("a", 10, 0));
    }
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn aggregation_error_rate_mixed() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    c.record(sample("a", 20, 1));
    c.record(sample("a", 30, 0));
    c.record(sample("a", 40, 1));
    let s = c.summary();
    // 2 total errors / 4 runs = 0.5
    assert!((s.error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn aggregation_after_clear_and_re_record() {
    let c = MetricsCollector::new();
    c.record(sample("old", 100, 5));
    c.clear();
    c.record(sample("new", 50, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.backend_counts["new"], 1);
    assert!(!s.backend_counts.contains_key("old"));
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 12. Custom tracing targets (abp.runtime, abp.sidecar, etc.)
// ===========================================================================

#[test]
fn custom_target_abp_runtime() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "abp.runtime", "runtime event");
    assert!(logs.contains("abp.runtime"), "logs: {}", logs.contents());
    assert!(logs.contains("runtime event"), "logs: {}", logs.contents());
}

#[test]
fn custom_target_abp_sidecar() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!(target: "abp.sidecar", run_id = "abc", "sidecar event");
    assert!(logs.contains("abp.sidecar"), "logs: {}", logs.contents());
    assert!(logs.contains("sidecar event"), "logs: {}", logs.contents());
}

#[test]
fn custom_target_abp_sidecar_stderr() {
    let (logs, _guard) = setup_tracing();
    tracing::warn!(target: "abp.sidecar.stderr", "stderr capture line");
    assert!(
        logs.contains("abp.sidecar.stderr"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn custom_target_abp_workspace() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!(target: "abp.workspace", src = "/tmp/src", "staging workspace");
    assert!(logs.contains("abp.workspace"), "logs: {}", logs.contents());
    assert!(
        logs.contains("staging workspace"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn multiple_targets_in_same_subscriber() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "abp.runtime", "from runtime");
    tracing::info!(target: "abp.sidecar", "from sidecar");
    tracing::info!(target: "abp.workspace", "from workspace");
    assert!(logs.contains("from runtime"), "logs: {}", logs.contents());
    assert!(logs.contains("from sidecar"), "logs: {}", logs.contents());
    assert!(logs.contains("from workspace"), "logs: {}", logs.contents());
}

// ===========================================================================
// 13. Metric serde roundtrip
// ===========================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = sample("serde_test", 999, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_default_serde_roundtrip() {
    let m = RunMetrics::default();
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(sample("a", 50, 1));
    c.record(sample("b", 100, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_default_serde_roundtrip() {
    let s = MetricsSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("run")
        .with_attribute("backend", "mock")
        .with_attribute("dialect", "openai");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["dialect"], "openai");
}

#[test]
fn telemetry_span_serde_preserves_attribute_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("zebra", "z")
        .with_attribute("alpha", "a");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    let keys: Vec<_> = span2.attributes.keys().collect();
    assert_eq!(keys, vec!["alpha", "zebra"]);
}

#[test]
fn run_metrics_json_field_names() {
    let m = sample("test", 42, 1);
    let json = serde_json::to_string(&m).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("backend_name").is_some());
    assert!(parsed.get("dialect").is_some());
    assert!(parsed.get("duration_ms").is_some());
    assert!(parsed.get("events_count").is_some());
    assert!(parsed.get("tokens_in").is_some());
    assert!(parsed.get("tokens_out").is_some());
    assert!(parsed.get("tool_calls_count").is_some());
    assert!(parsed.get("errors_count").is_some());
    assert!(parsed.get("emulations_applied").is_some());
}

// ===========================================================================
// 14. Span metadata preservation
// ===========================================================================

#[test]
fn span_name_preserved_through_clone() {
    let span = TelemetrySpan::new("preserved");
    let cloned = span.clone();
    assert_eq!(cloned.name, "preserved");
}

#[test]
fn span_attributes_preserved_through_clone() {
    let span = TelemetrySpan::new("op")
        .with_attribute("k1", "v1")
        .with_attribute("k2", "v2");
    let cloned = span.clone();
    assert_eq!(cloned.attributes.len(), 2);
    assert_eq!(cloned.attributes["k1"], "v1");
    assert_eq!(cloned.attributes["k2"], "v2");
}

#[test]
fn span_debug_format_contains_name() {
    let span = TelemetrySpan::new("debug_test");
    let debug_str = format!("{:?}", span);
    assert!(debug_str.contains("debug_test"));
}

#[test]
fn span_debug_format_contains_attributes() {
    let span = TelemetrySpan::new("op").with_attribute("backend", "mock");
    let debug_str = format!("{:?}", span);
    assert!(debug_str.contains("backend"));
    assert!(debug_str.contains("mock"));
}

#[test]
fn span_emit_includes_attributes_in_trace() {
    let (logs, _guard) = setup_tracing();

    let span = TelemetrySpan::new("emit_attrs")
        .with_attribute("tier", "premium")
        .with_attribute("region", "us-east");
    span.emit();

    assert!(logs.contains("emit_attrs"), "logs: {}", logs.contents());
    assert!(logs.contains("tier"), "logs: {}", logs.contents());
    assert!(logs.contains("region"), "logs: {}", logs.contents());
}

// ===========================================================================
// 15. Error span recording
// ===========================================================================

#[test]
fn error_event_captured_in_tracing() {
    let (logs, _guard) = setup_tracing();
    tracing::error!(error_code = "E001", "backend failed");
    assert!(logs.contains("backend failed"), "logs: {}", logs.contents());
    assert!(logs.contains("E001"), "logs: {}", logs.contents());
}

#[test]
fn error_within_span_context() {
    let (logs, _guard) = setup_tracing();
    let _span = tracing::error_span!("failing_operation", task = "sidecar_run").entered();
    tracing::error!("operation failed: timeout");
    assert!(
        logs.contains("failing_operation"),
        "logs: {}",
        logs.contents()
    );
    assert!(
        logs.contains("operation failed: timeout"),
        "logs: {}",
        logs.contents()
    );
}

#[test]
fn error_metrics_tracked() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 3));
    c.record(sample("b", 20, 0));
    c.record(sample("c", 30, 2));
    let s = c.summary();
    // total errors: 3 + 0 + 2 = 5, runs = 3 → rate = 5/3
    let expected_rate = 5.0 / 3.0;
    assert!((s.error_rate - expected_rate).abs() < f64::EPSILON);
}

#[test]
fn error_span_with_custom_target() {
    let (logs, _guard) = setup_tracing();
    tracing::error!(target: "abp.runtime", err = "connection_refused", "backend error");
    assert!(logs.contains("abp.runtime"), "logs: {}", logs.contents());
    assert!(
        logs.contains("connection_refused"),
        "logs: {}",
        logs.contents()
    );
}

// ===========================================================================
// Extra: JsonExporter integration
// ===========================================================================

#[test]
fn json_exporter_roundtrip() {
    let c = MetricsCollector::new();
    c.record(sample("mock", 100, 1));
    c.record(sample("openai", 200, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn json_exporter_empty_summary() {
    let s = MetricsSummary::default();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
}

#[test]
fn json_exporter_deterministic_key_ordering() {
    let c = MetricsCollector::new();
    c.record(sample("zebra", 10, 0));
    c.record(sample("alpha", 20, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < zebra_pos);
}

// ===========================================================================
// Extra: Collector edge cases
// ===========================================================================

#[test]
fn collector_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_clone_shares_state() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    c.record(sample("a", 10, 0));
    assert_eq!(c2.len(), 1);
}

#[test]
fn summary_backend_counts_uses_btreemap() {
    let c = MetricsCollector::new();
    c.record(sample("z", 10, 0));
    c.record(sample("a", 20, 0));
    c.record(sample("m", 30, 0));
    let s = c.summary();
    let keys: Vec<_> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn run_metrics_all_fields_independent() {
    let m = custom_metrics("backend", 100, 200, 300, 10, 5, 2, 1);
    assert_eq!(m.backend_name, "backend");
    assert_eq!(m.duration_ms, 100);
    assert_eq!(m.tokens_in, 200);
    assert_eq!(m.tokens_out, 300);
    assert_eq!(m.events_count, 10);
    assert_eq!(m.tool_calls_count, 5);
    assert_eq!(m.errors_count, 2);
    assert_eq!(m.emulations_applied, 1);
    assert_eq!(m.dialect, "custom");
}

#[test]
fn metrics_summary_default_has_empty_backend_counts() {
    let s = MetricsSummary::default();
    assert!(s.backend_counts.is_empty());
}
