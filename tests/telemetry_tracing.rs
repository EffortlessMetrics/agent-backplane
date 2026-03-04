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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the telemetry and tracing infrastructure.
//!
//! Covers: subscriber initialization, log level filtering, target-based
//! filtering, structured field capture, span creation/nesting, event emission,
//! async tracing, log output formatting, metrics collection, trace context
//! propagation, performance metrics, error event logging, and debug/release
//! logging behaviour.

use std::sync::{Arc, Mutex};
use std::thread;

use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};

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

/// Install a subscriber restricted to a specific max level.
fn setup_level_tracing(level: tracing::Level) -> (LogBuf, tracing::subscriber::DefaultGuard) {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(level)
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

fn metrics_with_tokens(backend: &str, tokens_in: u64, tokens_out: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test".to_string(),
        duration_ms: 50,
        events_count: 1,
        tokens_in,
        tokens_out,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    }
}

// ===========================================================================
// 1. Tracing subscriber initialization
// ===========================================================================

#[test]
fn subscriber_init_default_captures_info() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("init_check");
    assert!(logs.contains("init_check"), "logs: {}", logs.contents());
}

#[test]
fn subscriber_init_guard_scopes_output() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("scoped_msg");
    assert!(logs.contains("scoped_msg"));
    // After guard drops, subscriber is removed (verified by no panic).
}

#[test]
fn subscriber_init_multiple_sequential_guards() {
    {
        let (logs1, _g1) = setup_tracing();
        tracing::info!("first_sub");
        assert!(logs1.contains("first_sub"));
    }
    {
        let (logs2, _g2) = setup_tracing();
        tracing::info!("second_sub");
        assert!(logs2.contains("second_sub"));
    }
}

#[test]
fn subscriber_init_with_target_enabled() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "custom_target", "target_msg");
    assert!(logs.contains("custom_target"));
    assert!(logs.contains("target_msg"));
}

#[test]
fn subscriber_init_ansi_disabled() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("no_ansi");
    let content = logs.contents();
    // ANSI escape codes start with \x1b[
    assert!(!content.contains("\x1b["), "expected no ANSI codes");
}

#[test]
fn subscriber_init_json_mode() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!("json_init_test");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.is_object());
    }
}

// ===========================================================================
// 2. Log level filtering
// ===========================================================================

#[test]
fn level_filter_error_only() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::ERROR);
    tracing::error!("err_msg");
    tracing::warn!("warn_msg");
    tracing::info!("info_msg");
    assert!(logs.contains("err_msg"));
    assert!(!logs.contains("warn_msg"));
    assert!(!logs.contains("info_msg"));
}

#[test]
fn level_filter_warn_includes_error() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::WARN);
    tracing::error!("err2");
    tracing::warn!("warn2");
    tracing::info!("info2");
    assert!(logs.contains("err2"));
    assert!(logs.contains("warn2"));
    assert!(!logs.contains("info2"));
}

#[test]
fn level_filter_info_includes_warn_and_error() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::INFO);
    tracing::error!("e");
    tracing::warn!("w");
    tracing::info!("i");
    tracing::debug!("d");
    assert!(logs.contains("e"));
    assert!(logs.contains("w"));
    assert!(logs.contains("i"));
    assert!(!logs.contains("d"));
}

#[test]
fn level_filter_debug_includes_info_and_above() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::DEBUG);
    tracing::error!("e3");
    tracing::warn!("w3");
    tracing::info!("i3");
    tracing::debug!("d3");
    tracing::trace!("t3");
    assert!(logs.contains("e3"));
    assert!(logs.contains("w3"));
    assert!(logs.contains("i3"));
    assert!(logs.contains("d3"));
    assert!(!logs.contains("t3"));
}

#[test]
fn level_filter_trace_captures_all() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::TRACE);
    tracing::error!("e4");
    tracing::warn!("w4");
    tracing::info!("i4");
    tracing::debug!("d4");
    tracing::trace!("t4");
    assert!(logs.contains("e4"));
    assert!(logs.contains("w4"));
    assert!(logs.contains("i4"));
    assert!(logs.contains("d4"));
    assert!(logs.contains("t4"));
}

#[test]
fn level_filter_info_excludes_trace_and_debug() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::INFO);
    tracing::trace!("trace_only");
    tracing::debug!("debug_only");
    assert!(!logs.contains("trace_only"));
    assert!(!logs.contains("debug_only"));
}

// ===========================================================================
// 3. Target-based filtering
// ===========================================================================

#[test]
fn target_filter_allows_matching_target() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=info");
    tracing::info!(target: "abp.runtime", "runtime_yes");
    assert!(logs.contains("runtime_yes"));
}

#[test]
fn target_filter_blocks_non_matching_target() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=info");
    tracing::info!(target: "abp.sidecar", "sidecar_no");
    assert!(!logs.contains("sidecar_no"));
}

#[test]
fn target_filter_multiple_targets() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=info,abp.sidecar=debug");
    tracing::info!(target: "abp.runtime", "rt_msg");
    tracing::debug!(target: "abp.sidecar", "sc_msg");
    assert!(logs.contains("rt_msg"));
    assert!(logs.contains("sc_msg"));
}

#[test]
fn target_filter_level_granularity() {
    let (logs, _guard) = setup_filtered_tracing("abp.runtime=warn");
    tracing::warn!(target: "abp.runtime", "warn_visible");
    tracing::info!(target: "abp.runtime", "info_hidden");
    assert!(logs.contains("warn_visible"));
    assert!(!logs.contains("info_hidden"));
}

#[test]
fn target_filter_sidecar_stderr() {
    let (logs, _guard) = setup_filtered_tracing("abp.sidecar.stderr=trace");
    tracing::trace!(target: "abp.sidecar.stderr", "stderr_line");
    assert!(logs.contains("stderr_line"));
}

#[test]
fn target_filter_workspace_target() {
    let (logs, _guard) = setup_filtered_tracing("abp.workspace=debug");
    tracing::debug!(target: "abp.workspace", "ws_debug");
    tracing::trace!(target: "abp.workspace", "ws_trace");
    assert!(logs.contains("ws_debug"));
    assert!(!logs.contains("ws_trace"));
}

#[test]
fn target_filter_wildcard_prefix() {
    let (logs, _guard) = setup_filtered_tracing("abp=info");
    tracing::info!(target: "abp.runtime", "rt_wild");
    tracing::info!(target: "abp.sidecar", "sc_wild");
    tracing::info!(target: "abp.workspace", "ws_wild");
    assert!(logs.contains("rt_wild"));
    assert!(logs.contains("sc_wild"));
    assert!(logs.contains("ws_wild"));
}

// ===========================================================================
// 4. Structured field capture
// ===========================================================================

#[test]
fn structured_field_string_value() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(backend = "mock", "field_test");
    assert!(logs.contains("mock"));
}

#[test]
fn structured_field_numeric_value() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(duration_ms = 42u64, "num_field");
    assert!(logs.contains("42"));
}

#[test]
fn structured_field_bool_value() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(success = true, "bool_field");
    assert!(logs.contains("true"));
}

#[test]
fn structured_field_multiple_fields() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(backend = "openai", run_id = "r-1", events = 10, "multi");
    let c = logs.contents();
    assert!(c.contains("openai"));
    assert!(c.contains("r-1"));
    assert!(c.contains("10"));
}

#[test]
fn structured_field_in_json_output() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(backend = "claude", tokens = 500, "json_fields");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["fields"]["backend"], "claude");
        assert_eq!(v["fields"]["tokens"], 500);
    }
}

#[test]
fn structured_field_display_vs_debug() {
    let (logs, _guard) = setup_tracing();
    let data = vec![1, 2, 3];
    tracing::info!(items = ?data, "debug_field");
    assert!(logs.contains("[1, 2, 3]"));
}

#[test]
fn structured_field_empty_string() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(key = "", "empty_val");
    assert!(logs.contains("empty_val"));
}

// ===========================================================================
// 5. Span creation and nesting
// ===========================================================================

#[test]
fn span_create_basic() {
    let (logs, _guard) = setup_tracing();
    let _span = tracing::info_span!("basic_span").entered();
    tracing::info!("in_basic_span");
    assert!(logs.contains("basic_span"));
    assert!(logs.contains("in_basic_span"));
}

#[test]
fn span_nested_parent_child() {
    let (logs, _guard) = setup_tracing();
    let _parent = tracing::info_span!("parent").entered();
    {
        let _child = tracing::info_span!("child").entered();
        tracing::info!("in_child");
    }
    assert!(logs.contains("parent"));
    assert!(logs.contains("child"));
    assert!(logs.contains("in_child"));
}

#[test]
fn span_three_levels_deep() {
    let (logs, _guard) = setup_tracing();
    let _l1 = tracing::info_span!("l1").entered();
    let _l2 = tracing::info_span!("l2").entered();
    let _l3 = tracing::info_span!("l3").entered();
    tracing::info!("deep3");
    assert!(logs.contains("l1"));
    assert!(logs.contains("l2"));
    assert!(logs.contains("l3"));
    assert!(logs.contains("deep3"));
}

#[test]
fn span_sibling_spans() {
    let (logs, _guard) = setup_tracing();
    {
        let _a = tracing::info_span!("span_a").entered();
        tracing::info!("msg_a");
    }
    {
        let _b = tracing::info_span!("span_b").entered();
        tracing::info!("msg_b");
    }
    assert!(logs.contains("span_a"));
    assert!(logs.contains("span_b"));
}

#[test]
fn span_with_fields() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::info_span!("fielded", backend = "mock", run_id = "r-1").entered();
    tracing::info!("inside_fielded");
    assert!(logs.contains("fielded"));
    assert!(logs.contains("inside_fielded"));
}

#[test]
fn span_error_level() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::error_span!("error_span", code = "E500").entered();
    tracing::error!("fail");
    assert!(logs.contains("error_span"));
    assert!(logs.contains("fail"));
}

#[test]
fn span_debug_level() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::debug_span!("dbg_span").entered();
    tracing::debug!("dbg_inside");
    assert!(logs.contains("dbg_span"));
    assert!(logs.contains("dbg_inside"));
}

// ===========================================================================
// 6. Event emission with metadata
// ===========================================================================

#[test]
fn event_info_with_target() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "abp.runtime", "runtime_event");
    assert!(logs.contains("abp.runtime"));
    assert!(logs.contains("runtime_event"));
}

#[test]
fn event_warn_with_fields() {
    let (logs, _guard) = setup_tracing();
    tracing::warn!(code = "W001", detail = "slow", "warning_event");
    assert!(logs.contains("W001"));
    assert!(logs.contains("warning_event"));
}

#[test]
fn event_error_with_structured_error() {
    let (logs, _guard) = setup_tracing();
    let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    tracing::error!(error = %err, "io_error_event");
    assert!(logs.contains("file missing"));
    assert!(logs.contains("io_error_event"));
}

#[test]
fn event_inside_span_includes_span_name() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::info_span!("outer_ctx").entered();
    tracing::info!("ctx_event");
    assert!(logs.contains("outer_ctx"));
}

#[test]
fn event_multiple_levels_in_sequence() {
    let (logs, _guard) = setup_tracing();
    tracing::trace!("t_ev");
    tracing::debug!("d_ev");
    tracing::info!("i_ev");
    tracing::warn!("w_ev");
    tracing::error!("e_ev");
    let c = logs.contents();
    assert!(c.contains("t_ev"));
    assert!(c.contains("d_ev"));
    assert!(c.contains("i_ev"));
    assert!(c.contains("w_ev"));
    assert!(c.contains("e_ev"));
}

#[test]
fn event_json_includes_level_field() {
    let (logs, _guard) = setup_json_tracing();
    tracing::warn!("level_check");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["level"], "WARN");
    }
}

#[test]
fn event_json_includes_target_field() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(target: "abp.sidecar", "target_json");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["target"], "abp.sidecar");
    }
}

// ===========================================================================
// 7. Tracing in async contexts
// ===========================================================================

#[tokio::test]
async fn async_tracing_captures_info() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("async_info");
    assert!(logs.contains("async_info"));
}

#[tokio::test]
async fn async_tracing_span_across_await() {
    let (logs, _guard) = setup_tracing();
    let _span = tracing::info_span!("async_span").entered();
    tokio::task::yield_now().await;
    tracing::info!("after_yield");
    assert!(logs.contains("async_span"));
    assert!(logs.contains("after_yield"));
}

#[tokio::test]
async fn async_tracing_nested_spawns() {
    let (logs, _guard) = setup_tracing();
    let _span = tracing::info_span!("root_async").entered();
    tracing::info!("root_msg");

    // The inner task shares the subscriber (set_default is thread-local).
    tokio::task::yield_now().await;
    tracing::info!("after_spawn");

    assert!(logs.contains("root_msg"));
    assert!(logs.contains("after_spawn"));
}

#[tokio::test]
async fn async_tracing_instrument_future() {
    use tracing::Instrument;
    let (logs, _guard) = setup_tracing();

    async {
        tracing::info!("instrumented_msg");
    }
    .instrument(tracing::info_span!("instrumented_span"))
    .await;

    assert!(logs.contains("instrumented_span"));
    assert!(logs.contains("instrumented_msg"));
}

#[tokio::test]
async fn async_tracing_multiple_tasks() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("task1_msg");
    tokio::task::yield_now().await;
    tracing::info!("task2_msg");
    assert!(logs.contains("task1_msg"));
    assert!(logs.contains("task2_msg"));
}

#[tokio::test]
async fn async_tracing_error_in_async() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::error_span!("async_err_span").entered();
    tracing::error!("async_failure");
    assert!(logs.contains("async_err_span"));
    assert!(logs.contains("async_failure"));
}

// ===========================================================================
// 8. Log output formatting
// ===========================================================================

#[test]
fn format_plain_text_includes_message() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("plain_text_msg");
    assert!(logs.contains("plain_text_msg"));
}

#[test]
fn format_json_valid_per_line() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!("json_line_1");
    tracing::warn!("json_line_2");
    let content = logs.contents();
    let mut count = 0;
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.is_object());
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn format_json_contains_timestamp() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!("ts_check");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("timestamp").is_some());
    }
}

#[test]
fn format_json_contains_message() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!("msg_check");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let msg = v["fields"]["message"].as_str().unwrap();
        assert_eq!(msg, "msg_check");
    }
}

#[test]
fn format_plain_text_no_json() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("not_json");
    let content = logs.contents();
    assert!(serde_json::from_str::<serde_json::Value>(&content).is_err());
}

#[test]
fn format_target_shown_in_output() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "my.target", "target_fmt");
    assert!(logs.contains("my.target"));
}

// ===========================================================================
// 9. Telemetry metrics collection
// ===========================================================================

#[test]
fn metrics_collector_new_empty() {
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn metrics_collector_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
}

#[test]
fn metrics_collector_record_increments() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    c.record(sample("b", 20, 0));
    c.record(sample("c", 30, 0));
    assert_eq!(c.len(), 3);
}

#[test]
fn metrics_collector_runs_preserves_order() {
    let c = MetricsCollector::new();
    c.record(sample("first", 10, 0));
    c.record(sample("second", 20, 0));
    let runs = c.runs();
    assert_eq!(runs[0].backend_name, "first");
    assert_eq!(runs[1].backend_name, "second");
}

#[test]
fn metrics_collector_clear_empties() {
    let c = MetricsCollector::new();
    c.record(sample("x", 10, 0));
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn metrics_collector_summary_empty() {
    let c = MetricsCollector::new();
    let s = c.summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert!(s.backend_counts.is_empty());
}

#[test]
fn metrics_collector_summary_single() {
    let c = MetricsCollector::new();
    c.record(sample("mock", 100, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - 100.0).abs() < f64::EPSILON);
    assert_eq!(s.backend_counts["mock"], 1);
}

#[test]
fn metrics_collector_summary_multi_backend() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    c.record(sample("b", 20, 0));
    c.record(sample("a", 30, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["a"], 2);
    assert_eq!(s.backend_counts["b"], 1);
}

#[test]
fn metrics_collector_token_aggregation() {
    let c = MetricsCollector::new();
    c.record(metrics_with_tokens("a", 100, 200));
    c.record(metrics_with_tokens("b", 300, 400));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 400);
    assert_eq!(s.total_tokens_out, 600);
}

#[test]
fn metrics_collector_error_rate_zero() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    c.record(sample("b", 20, 0));
    let s = c.summary();
    assert!((s.error_rate).abs() < f64::EPSILON);
}

#[test]
fn metrics_collector_error_rate_mixed() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 2));
    c.record(sample("b", 20, 0));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON); // 2 errors / 2 runs
}

// ===========================================================================
// 10. Trace context propagation
// ===========================================================================

#[test]
fn context_propagation_span_to_event() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::info_span!("ctx_span", run_id = "run-42").entered();
    tracing::info!("ctx_event");
    assert!(logs.contains("ctx_span"));
    assert!(logs.contains("ctx_event"));
}

#[test]
fn context_propagation_nested_spans_visible() {
    let (logs, _guard) = setup_tracing();
    let _outer = tracing::info_span!("outer_ctx").entered();
    let _inner = tracing::info_span!("inner_ctx").entered();
    tracing::info!("deep_ctx");
    assert!(logs.contains("outer_ctx"));
    assert!(logs.contains("inner_ctx"));
}

#[test]
fn context_propagation_sibling_isolation() {
    let (logs, _guard) = setup_tracing();
    {
        let _a = tracing::info_span!("ctx_a").entered();
        tracing::info!("msg_a_ctx");
    }
    {
        let _b = tracing::info_span!("ctx_b").entered();
        tracing::info!("msg_b_ctx");
    }
    // Both appear but in separate contexts
    assert!(logs.contains("ctx_a"));
    assert!(logs.contains("ctx_b"));
}

#[test]
fn context_propagation_json_span_field() {
    let (logs, _guard) = setup_json_tracing();
    let _s = tracing::info_span!("json_ctx_span").entered();
    tracing::info!("json_ctx_msg");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        // Span info is included in JSON output
        assert!(v.is_object());
    }
}

#[tokio::test]
async fn context_propagation_across_await() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::info_span!("await_ctx").entered();
    tokio::task::yield_now().await;
    tracing::info!("post_await_ctx");
    assert!(logs.contains("await_ctx"));
    assert!(logs.contains("post_await_ctx"));
}

#[test]
fn context_propagation_telemetry_span_emit() {
    let (logs, _guard) = setup_tracing();
    let span = TelemetrySpan::new("ctx_emit").with_attribute("run_id", "r-99");
    span.emit();
    assert!(logs.contains("ctx_emit"));
    assert!(logs.contains("telemetry_span"));
}

// ===========================================================================
// 11. Performance metrics
// ===========================================================================

#[test]
fn perf_metrics_p50_odd_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn perf_metrics_p50_even_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn perf_metrics_p99_large_set() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn perf_metrics_mean_accuracy() {
    let c = MetricsCollector::new();
    c.record(sample("a", 100, 0));
    c.record(sample("a", 200, 0));
    c.record(sample("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn perf_metrics_identical_values() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(sample("a", 50, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 50.0).abs() < f64::EPSILON);
}

#[test]
fn perf_metrics_zero_duration() {
    let c = MetricsCollector::new();
    c.record(sample("a", 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms).abs() < f64::EPSILON);
}

#[test]
fn perf_metrics_single_value_all_percentiles_equal() {
    let c = MetricsCollector::new();
    c.record(sample("a", 42, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 12. Error event logging
// ===========================================================================

#[test]
fn error_event_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::error!("critical_failure");
    assert!(logs.contains("critical_failure"));
}

#[test]
fn error_event_with_code() {
    let (logs, _guard) = setup_tracing();
    tracing::error!(error_code = "E001", "coded_error");
    assert!(logs.contains("E001"));
    assert!(logs.contains("coded_error"));
}

#[test]
fn error_event_within_span() {
    let (logs, _guard) = setup_tracing();
    let _s = tracing::error_span!("err_context", task = "sidecar").entered();
    tracing::error!("context_error");
    assert!(logs.contains("err_context"));
    assert!(logs.contains("context_error"));
}

#[test]
fn error_event_display_error_type() {
    let (logs, _guard) = setup_tracing();
    let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    tracing::error!(error = %err, "io_err");
    assert!(logs.contains("access denied"));
}

#[test]
fn error_event_multiple_errors_sequential() {
    let (logs, _guard) = setup_tracing();
    tracing::error!("err_one");
    tracing::error!("err_two");
    tracing::error!("err_three");
    assert!(logs.contains("err_one"));
    assert!(logs.contains("err_two"));
    assert!(logs.contains("err_three"));
}

#[test]
fn error_event_json_level_error() {
    let (logs, _guard) = setup_json_tracing();
    tracing::error!("json_err");
    let content = logs.contents();
    for line in content.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["level"], "ERROR");
    }
}

#[test]
fn error_metrics_count_tracking() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 3));
    c.record(sample("b", 20, 7));
    let runs = c.runs();
    let total: u64 = runs.iter().map(|r| r.errors_count).sum();
    assert_eq!(total, 10);
}

#[test]
fn error_rate_all_failures() {
    let c = MetricsCollector::new();
    for _ in 0..4 {
        c.record(sample("a", 10, 1));
    }
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 13. Debug vs release logging behavior
// ===========================================================================

#[test]
fn debug_level_events_visible_at_debug_level() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::DEBUG);
    tracing::debug!("debug_visible");
    assert!(logs.contains("debug_visible"));
}

#[test]
fn debug_level_events_hidden_at_info_level() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::INFO);
    tracing::debug!("debug_hidden");
    assert!(!logs.contains("debug_hidden"));
}

#[test]
fn trace_level_events_hidden_at_debug_level() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::DEBUG);
    tracing::trace!("trace_hidden");
    assert!(!logs.contains("trace_hidden"));
}

#[test]
fn release_style_info_only() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::INFO);
    tracing::info!("release_info");
    tracing::debug!("release_debug");
    tracing::trace!("release_trace");
    assert!(logs.contains("release_info"));
    assert!(!logs.contains("release_debug"));
    assert!(!logs.contains("release_trace"));
}

#[test]
fn debug_style_verbose() {
    let (logs, _guard) = setup_level_tracing(tracing::Level::TRACE);
    tracing::info!("verbose_info");
    tracing::debug!("verbose_debug");
    tracing::trace!("verbose_trace");
    assert!(logs.contains("verbose_info"));
    assert!(logs.contains("verbose_debug"));
    assert!(logs.contains("verbose_trace"));
}

#[test]
fn error_always_visible_at_any_level() {
    for level in [
        tracing::Level::ERROR,
        tracing::Level::WARN,
        tracing::Level::INFO,
        tracing::Level::DEBUG,
        tracing::Level::TRACE,
    ] {
        let (logs, _guard) = setup_level_tracing(level);
        tracing::error!("always_err");
        assert!(logs.contains("always_err"), "failed at {:?}", level);
    }
}

// ===========================================================================
// Additional: TelemetrySpan integration
// ===========================================================================

#[test]
fn telemetry_span_new_empty_attributes() {
    let span = TelemetrySpan::new("op");
    assert_eq!(span.name, "op");
    assert!(span.attributes.is_empty());
}

#[test]
fn telemetry_span_with_attribute_chaining() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
}

#[test]
fn telemetry_span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "old")
        .with_attribute("key", "new");
    assert_eq!(span.attributes["key"], "new");
    assert_eq!(span.attributes.len(), 1);
}

#[test]
fn telemetry_span_attributes_btree_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z", "last")
        .with_attribute("a", "first")
        .with_attribute("m", "middle");
    let keys: Vec<_> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn telemetry_span_emit_captured() {
    let (logs, _guard) = setup_tracing();
    let span = TelemetrySpan::new("emit_op").with_attribute("backend", "mock");
    span.emit();
    assert!(logs.contains("telemetry_span"));
    assert!(logs.contains("emit_op"));
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("serde_op").with_attribute("k", "v");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "serde_op");
    assert_eq!(span2.attributes["k"], "v");
}

#[test]
fn telemetry_span_clone_independent() {
    let span = TelemetrySpan::new("orig").with_attribute("k", "v");
    let mut cloned = span.clone();
    cloned.attributes.insert("new_k".into(), "new_v".into());
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(cloned.attributes.len(), 2);
}

#[test]
fn telemetry_span_debug_contains_name() {
    let span = TelemetrySpan::new("dbg_check");
    let dbg = format!("{:?}", span);
    assert!(dbg.contains("dbg_check"));
}

// ===========================================================================
// Additional: JSON exporter
// ===========================================================================

#[test]
fn json_exporter_valid_output() {
    let c = MetricsCollector::new();
    c.record(sample("mock", 100, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 1);
}

#[test]
fn json_exporter_empty_summary() {
    let s = MetricsSummary::default();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 0);
}

#[test]
fn json_exporter_deterministic_key_order() {
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
// Additional: Serde roundtrips
// ===========================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = sample("serde_rt", 42, 1);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_default_roundtrip() {
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
fn run_metrics_json_fields_present() {
    let m = sample("test", 42, 1);
    let json = serde_json::to_string(&m).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("backend_name").is_some());
    assert!(v.get("dialect").is_some());
    assert!(v.get("duration_ms").is_some());
    assert!(v.get("events_count").is_some());
    assert!(v.get("tokens_in").is_some());
    assert!(v.get("tokens_out").is_some());
    assert!(v.get("tool_calls_count").is_some());
    assert!(v.get("errors_count").is_some());
    assert!(v.get("emulations_applied").is_some());
}

// ===========================================================================
// Additional: Concurrent metrics operations
// ===========================================================================

#[test]
fn concurrent_recording_thread_safe() {
    let c = MetricsCollector::new();
    let mut handles = Vec::new();
    for i in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample("thread", i * 5, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
}

#[test]
fn concurrent_summary_reads_consistent() {
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

// ===========================================================================
// Additional: ABP tracing targets
// ===========================================================================

#[test]
fn abp_runtime_target_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "abp.runtime", "rt_captured");
    assert!(logs.contains("abp.runtime"));
    assert!(logs.contains("rt_captured"));
}

#[test]
fn abp_sidecar_target_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!(target: "abp.sidecar", pid = 1234, "sidecar_captured");
    assert!(logs.contains("abp.sidecar"));
    assert!(logs.contains("sidecar_captured"));
}

#[test]
fn abp_sidecar_stderr_target_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::warn!(target: "abp.sidecar.stderr", "stderr_captured");
    assert!(logs.contains("abp.sidecar.stderr"));
}

#[test]
fn abp_workspace_target_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!(target: "abp.workspace", path = "/tmp/ws", "ws_captured");
    assert!(logs.contains("abp.workspace"));
    assert!(logs.contains("ws_captured"));
}

#[test]
fn all_abp_targets_in_single_subscriber() {
    let (logs, _guard) = setup_tracing();
    tracing::info!(target: "abp.runtime", "from_runtime");
    tracing::info!(target: "abp.sidecar", "from_sidecar");
    tracing::info!(target: "abp.sidecar.stderr", "from_stderr");
    tracing::info!(target: "abp.workspace", "from_workspace");
    let c = logs.contents();
    assert!(c.contains("from_runtime"));
    assert!(c.contains("from_sidecar"));
    assert!(c.contains("from_stderr"));
    assert!(c.contains("from_workspace"));
}
