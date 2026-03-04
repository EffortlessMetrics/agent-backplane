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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for `abp-telemetry`: RunMetrics, MetricsCollector,
//! MetricsSummary, TelemetrySpan, JsonExporter, concurrent operations,
//! serialization, aggregation math, and tracing integration.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;

use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};

// ===========================================================================
// Log-capture infrastructure
// ===========================================================================

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

// ===========================================================================
// Helpers
// ===========================================================================

fn mk(backend: &str, duration: u64, errors: u64) -> RunMetrics {
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

fn mk_tokens(backend: &str, tok_in: u64, tok_out: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "d".to_string(),
        duration_ms: 1,
        events_count: 0,
        tokens_in: tok_in,
        tokens_out: tok_out,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn mk_full(
    backend: &str,
    dialect: &str,
    duration: u64,
    events: u64,
    tok_in: u64,
    tok_out: u64,
    tools: u64,
    errors: u64,
    emulations: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: dialect.to_string(),
        duration_ms: duration,
        events_count: events,
        tokens_in: tok_in,
        tokens_out: tok_out,
        tool_calls_count: tools,
        errors_count: errors,
        emulations_applied: emulations,
    }
}

// ===========================================================================
// 1. RunMetrics construction and types
// ===========================================================================

#[test]
fn dc_run_metrics_default_is_zeroed() {
    let m = RunMetrics::default();
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
fn dc_run_metrics_field_assignment() {
    let m = mk("mock", 42, 1);
    assert_eq!(m.backend_name, "mock");
    assert_eq!(m.duration_ms, 42);
    assert_eq!(m.errors_count, 1);
}

#[test]
fn dc_run_metrics_clone_equals_original() {
    let m = mk("a", 100, 0);
    assert_eq!(m.clone(), m);
}

#[test]
fn dc_run_metrics_partial_eq_different_backend() {
    assert_ne!(mk("a", 100, 0), mk("b", 100, 0));
}

#[test]
fn dc_run_metrics_partial_eq_different_duration() {
    assert_ne!(mk("a", 100, 0), mk("a", 200, 0));
}

#[test]
fn dc_run_metrics_partial_eq_different_errors() {
    assert_ne!(mk("a", 100, 0), mk("a", 100, 1));
}

#[test]
fn dc_run_metrics_debug_format() {
    let dbg = format!("{:?}", RunMetrics::default());
    assert!(dbg.contains("RunMetrics"));
}

#[test]
fn dc_run_metrics_all_fields_populated() {
    let m = mk_full("b", "d", 1, 2, 3, 4, 5, 6, 7);
    assert_eq!(m.backend_name, "b");
    assert_eq!(m.dialect, "d");
    assert_eq!(m.duration_ms, 1);
    assert_eq!(m.events_count, 2);
    assert_eq!(m.tokens_in, 3);
    assert_eq!(m.tokens_out, 4);
    assert_eq!(m.tool_calls_count, 5);
    assert_eq!(m.errors_count, 6);
    assert_eq!(m.emulations_applied, 7);
}

#[test]
fn dc_run_metrics_with_large_values() {
    let m = RunMetrics {
        backend_name: "big".into(),
        dialect: "d".into(),
        duration_ms: u64::MAX,
        events_count: u64::MAX,
        tokens_in: u64::MAX,
        tokens_out: u64::MAX,
        tool_calls_count: u64::MAX,
        errors_count: u64::MAX,
        emulations_applied: u64::MAX,
    };
    assert_eq!(m.duration_ms, u64::MAX);
}

#[test]
fn dc_run_metrics_unicode_backend_name() {
    let m = mk_full("bäckénd-日本語", "диалект", 10, 0, 0, 0, 0, 0, 0);
    assert_eq!(m.backend_name, "bäckénd-日本語");
    assert_eq!(m.dialect, "диалект");
}

#[test]
fn dc_run_metrics_empty_string_fields() {
    let m = mk_full("", "", 0, 0, 0, 0, 0, 0, 0);
    assert_eq!(m.backend_name, "");
    assert_eq!(m.dialect, "");
}

// ===========================================================================
// 2. RunMetrics serialization
// ===========================================================================

#[test]
fn dc_run_metrics_serde_roundtrip_default() {
    let m = RunMetrics::default();
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn dc_run_metrics_serde_roundtrip_populated() {
    let m = mk("serde", 999, 3);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn dc_run_metrics_json_has_all_keys() {
    let v: serde_json::Value = serde_json::to_value(&mk("k", 1, 0)).unwrap();
    for key in [
        "backend_name",
        "dialect",
        "duration_ms",
        "events_count",
        "tokens_in",
        "tokens_out",
        "tool_calls_count",
        "errors_count",
        "emulations_applied",
    ] {
        assert!(v.get(key).is_some(), "missing key: {key}");
    }
}

#[test]
fn dc_run_metrics_deserialize_manual_json() {
    let json = r#"{
        "backend_name": "manual",
        "dialect": "d",
        "duration_ms": 77,
        "events_count": 1,
        "tokens_in": 2,
        "tokens_out": 3,
        "tool_calls_count": 4,
        "errors_count": 5,
        "emulations_applied": 6
    }"#;
    let m: RunMetrics = serde_json::from_str(json).unwrap();
    assert_eq!(m.backend_name, "manual");
    assert_eq!(m.duration_ms, 77);
}

#[test]
fn dc_run_metrics_pretty_roundtrip() {
    let m = mk("pretty", 123, 0);
    let json = serde_json::to_string_pretty(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn dc_run_metrics_serde_large_values() {
    let m = RunMetrics {
        backend_name: "large".into(),
        dialect: "d".into(),
        duration_ms: u64::MAX,
        events_count: u64::MAX,
        tokens_in: u64::MAX,
        tokens_out: u64::MAX,
        tool_calls_count: u64::MAX,
        errors_count: u64::MAX,
        emulations_applied: u64::MAX,
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn dc_run_metrics_value_roundtrip() {
    let m = mk("val", 42, 0);
    let v = serde_json::to_value(&m).unwrap();
    let m2: RunMetrics = serde_json::from_value(v).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn dc_run_metrics_json_numeric_types() {
    let v: serde_json::Value = serde_json::to_value(&mk("n", 42, 0)).unwrap();
    assert!(v["duration_ms"].is_u64());
    assert!(v["tokens_in"].is_u64());
    assert!(v["errors_count"].is_u64());
}

// ===========================================================================
// 3. MetricsSummary defaults and types
// ===========================================================================

#[test]
fn dc_summary_default_all_zero() {
    let s = MetricsSummary::default();
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
fn dc_summary_clone_eq() {
    let s = MetricsSummary::default();
    assert_eq!(s.clone(), s);
}

#[test]
fn dc_summary_debug_format() {
    let dbg = format!("{:?}", MetricsSummary::default());
    assert!(dbg.contains("MetricsSummary"));
}

#[test]
fn dc_summary_serde_roundtrip_default() {
    let s = MetricsSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn dc_summary_serde_roundtrip_populated() {
    let c = MetricsCollector::new();
    c.record(mk("a", 100, 1));
    c.record(mk("b", 200, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn dc_summary_json_keys_present() {
    let v: serde_json::Value = serde_json::to_value(&MetricsSummary::default()).unwrap();
    for key in [
        "count",
        "mean_duration_ms",
        "p50_duration_ms",
        "p99_duration_ms",
        "total_tokens_in",
        "total_tokens_out",
        "error_rate",
        "backend_counts",
    ] {
        assert!(v.get(key).is_some(), "missing key: {key}");
    }
}

#[test]
fn dc_summary_value_roundtrip() {
    let c = MetricsCollector::new();
    c.record(mk("v", 50, 0));
    let s = c.summary();
    let v = serde_json::to_value(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_value(v).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn dc_summary_backend_counts_is_btreemap() {
    let c = MetricsCollector::new();
    c.record(mk("zebra", 1, 0));
    c.record(mk("apple", 2, 0));
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["apple", "zebra"]);
}

// ===========================================================================
// 4. MetricsCollector basic operations
// ===========================================================================

#[test]
fn dc_collector_new_is_empty() {
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn dc_collector_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
}

#[test]
fn dc_collector_record_increments() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    assert_eq!(c.len(), 1);
    c.record(mk("b", 20, 0));
    assert_eq!(c.len(), 2);
}

#[test]
fn dc_collector_not_empty_after_record() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 0));
    assert!(!c.is_empty());
}

#[test]
fn dc_collector_runs_preserves_order() {
    let c = MetricsCollector::new();
    c.record(mk("first", 1, 0));
    c.record(mk("second", 2, 0));
    c.record(mk("third", 3, 0));
    let r = c.runs();
    assert_eq!(r[0].backend_name, "first");
    assert_eq!(r[1].backend_name, "second");
    assert_eq!(r[2].backend_name, "third");
}

#[test]
fn dc_collector_runs_returns_clone() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 0));
    assert_eq!(c.runs(), c.runs());
}

#[test]
fn dc_collector_clear_empties() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 0));
    c.record(mk("b", 2, 0));
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn dc_collector_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 0));
    c.clear();
    c.record(mk("b", 2, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

#[test]
fn dc_collector_clone_shares_state() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    c.record(mk("shared", 1, 0));
    assert_eq!(c2.len(), 1);
}

#[test]
fn dc_collector_debug_format() {
    let dbg = format!("{:?}", MetricsCollector::new());
    assert!(dbg.contains("MetricsCollector"));
}

#[test]
fn dc_collector_many_records() {
    let c = MetricsCollector::new();
    for i in 0..500 {
        c.record(mk("bulk", i, 0));
    }
    assert_eq!(c.len(), 500);
}

// ===========================================================================
// 5. MetricsCollector summary – single run
// ===========================================================================

#[test]
fn dc_summary_single_run_count() {
    let c = MetricsCollector::new();
    c.record(mk("x", 50, 0));
    assert_eq!(c.summary().count, 1);
}

#[test]
fn dc_summary_single_run_mean_eq_duration() {
    let c = MetricsCollector::new();
    c.record(mk("x", 42, 0));
    assert!((c.summary().mean_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_single_run_p50_eq_duration() {
    let c = MetricsCollector::new();
    c.record(mk("x", 42, 0));
    assert!((c.summary().p50_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_single_run_p99_eq_duration() {
    let c = MetricsCollector::new();
    c.record(mk("x", 42, 0));
    assert!((c.summary().p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_single_run_tokens() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 100);
    assert_eq!(s.total_tokens_out, 200);
}

#[test]
fn dc_summary_single_run_zero_errors() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 0));
    assert!((c.summary().error_rate).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_single_run_with_errors() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 5));
    assert!((c.summary().error_rate - 5.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_single_run_backend_count() {
    let c = MetricsCollector::new();
    c.record(mk("myback", 1, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["myback"], 1);
}

// ===========================================================================
// 6. Metric aggregation and summary
// ===========================================================================

#[test]
fn dc_summary_mean_two_runs() {
    let c = MetricsCollector::new();
    c.record(mk("a", 100, 0));
    c.record(mk("a", 200, 0));
    assert!((c.summary().mean_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_mean_three_runs() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    c.record(mk("a", 20, 0));
    c.record(mk("a", 30, 0));
    assert!((c.summary().mean_duration_ms - 20.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_p50_odd() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(mk("a", d, 0));
    }
    assert!((c.summary().p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_p50_even() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(mk("a", d, 0));
    }
    assert!((c.summary().p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_p50_two_runs() {
    let c = MetricsCollector::new();
    c.record(mk("a", 100, 0));
    c.record(mk("a", 200, 0));
    assert!((c.summary().p50_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_p99_100_runs() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(mk("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn dc_summary_p99_two_runs() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    c.record(mk("a", 1000, 0));
    let s = c.summary();
    assert!((s.p99_duration_ms - 990.1).abs() < 0.01);
}

#[test]
fn dc_summary_identical_durations() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(mk("a", 100, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 100.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 100.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 100.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_tokens_in_aggregated() {
    let c = MetricsCollector::new();
    c.record(mk_tokens("a", 100, 0));
    c.record(mk_tokens("b", 250, 0));
    assert_eq!(c.summary().total_tokens_in, 350);
}

#[test]
fn dc_summary_tokens_out_aggregated() {
    let c = MetricsCollector::new();
    c.record(mk_tokens("a", 0, 300));
    c.record(mk_tokens("b", 0, 700));
    assert_eq!(c.summary().total_tokens_out, 1000);
}

#[test]
fn dc_summary_error_rate_mixed() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 0));
    c.record(mk("a", 2, 1));
    c.record(mk("a", 3, 0));
    c.record(mk("a", 4, 1));
    assert!((c.summary().error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 1));
    c.record(mk("a", 2, 1));
    assert!((c.summary().error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 0));
    c.record(mk("a", 2, 0));
    assert!((c.summary().error_rate).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_error_rate_multi_per_run() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 3));
    c.record(mk("a", 2, 0));
    // 3 errors / 2 runs = 1.5
    assert!((c.summary().error_rate - 1.5).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_backend_counts_multiple() {
    let c = MetricsCollector::new();
    c.record(mk("alpha", 1, 0));
    c.record(mk("beta", 2, 0));
    c.record(mk("alpha", 3, 0));
    c.record(mk("gamma", 4, 0));
    c.record(mk("beta", 5, 0));
    c.record(mk("beta", 6, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 3);
    assert_eq!(s.backend_counts["gamma"], 1);
}

#[test]
fn dc_summary_backend_counts_deterministic() {
    let c = MetricsCollector::new();
    c.record(mk("zebra", 1, 0));
    c.record(mk("apple", 2, 0));
    c.record(mk("mango", 3, 0));
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["apple", "mango", "zebra"]);
}

// ===========================================================================
// 7. Empty / cleared collector summary
// ===========================================================================

#[test]
fn dc_empty_collector_summary_is_default() {
    assert_eq!(MetricsCollector::new().summary(), MetricsSummary::default());
}

#[test]
fn dc_cleared_collector_summary_is_default() {
    let c = MetricsCollector::new();
    c.record(mk("a", 100, 1));
    c.clear();
    assert_eq!(c.summary(), MetricsSummary::default());
}

#[test]
fn dc_double_clear_is_idempotent() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 0));
    c.clear();
    c.clear();
    assert!(c.is_empty());
}

// ===========================================================================
// 8. Performance metrics (latency, throughput proxies)
// ===========================================================================

#[test]
fn dc_latency_ordering_preserved_in_p50() {
    let c = MetricsCollector::new();
    // reverse order: durations are sorted internally
    for d in [50, 40, 30, 20, 10] {
        c.record(mk("a", d, 0));
    }
    assert!((c.summary().p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn dc_throughput_proxy_tokens_sum() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(mk_tokens("a", 50, 75));
    }
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 500);
    assert_eq!(s.total_tokens_out, 750);
}

#[test]
fn dc_latency_spread_mean_vs_p50() {
    let c = MetricsCollector::new();
    // One outlier
    c.record(mk("a", 10, 0));
    c.record(mk("a", 10, 0));
    c.record(mk("a", 10, 0));
    c.record(mk("a", 10, 0));
    c.record(mk("a", 10000, 0)); // massive outlier
    let s = c.summary();
    // Mean is heavily influenced by outlier
    assert!(s.mean_duration_ms > 2000.0);
    // p50 is not
    assert!(s.p50_duration_ms < 100.0);
}

#[test]
fn dc_p99_large_dataset() {
    let c = MetricsCollector::new();
    for d in 1..=1000 {
        c.record(mk("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 989.0);
    assert!(s.p99_duration_ms <= 1000.0);
}

// ===========================================================================
// 9. Error metric tracking
// ===========================================================================

#[test]
fn dc_error_tracking_per_run() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 2));
    c.record(mk("b", 20, 3));
    let runs = c.runs();
    let total: u64 = runs.iter().map(|r| r.errors_count).sum();
    assert_eq!(total, 5);
}

#[test]
fn dc_error_count_zero_for_clean_runs() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(mk("a", 1, 0));
    }
    let total: u64 = c.runs().iter().map(|r| r.errors_count).sum();
    assert_eq!(total, 0);
    assert!(c.summary().error_rate.abs() < f64::EPSILON);
}

#[test]
fn dc_error_rate_high_error_count() {
    let c = MetricsCollector::new();
    c.record(mk("a", 1, 100));
    // 100 errors / 1 run = 100.0
    assert!((c.summary().error_rate - 100.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 10. Custom metrics and labels (RunMetrics fields as labels)
// ===========================================================================

#[test]
fn dc_custom_dialect_label() {
    let m = mk_full("backend", "anthropic/claude-3", 50, 10, 100, 200, 5, 0, 2);
    assert_eq!(m.dialect, "anthropic/claude-3");
}

#[test]
fn dc_emulation_count_label() {
    let m = mk_full("b", "d", 1, 0, 0, 0, 0, 0, 42);
    assert_eq!(m.emulations_applied, 42);
}

#[test]
fn dc_tool_calls_count_label() {
    let m = mk_full("b", "d", 1, 0, 0, 0, 99, 0, 0);
    assert_eq!(m.tool_calls_count, 99);
}

#[test]
fn dc_events_count_label() {
    let m = mk_full("b", "d", 1, 1234, 0, 0, 0, 0, 0);
    assert_eq!(m.events_count, 1234);
}

#[test]
fn dc_metric_naming_backend_counts_key() {
    let c = MetricsCollector::new();
    c.record(mk("my-backend-v2", 1, 0));
    assert!(c.summary().backend_counts.contains_key("my-backend-v2"));
}

#[test]
fn dc_metric_naming_with_special_chars() {
    let c = MetricsCollector::new();
    c.record(mk("backend/v1.2.3", 1, 0));
    assert!(c.summary().backend_counts.contains_key("backend/v1.2.3"));
}

// ===========================================================================
// 11. TelemetrySpan creation and nesting
// ===========================================================================

#[test]
fn dc_span_basic_creation() {
    let span = TelemetrySpan::new("test-op");
    assert_eq!(span.name, "test-op");
    assert!(span.attributes.is_empty());
}

#[test]
fn dc_span_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn dc_span_from_string_type() {
    let name = String::from("dynamic");
    let span = TelemetrySpan::new(name);
    assert_eq!(span.name, "dynamic");
}

#[test]
fn dc_span_single_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("backend", "mock");
    assert_eq!(span.attributes["backend"], "mock");
}

#[test]
fn dc_span_multiple_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
}

#[test]
fn dc_span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "old")
        .with_attribute("key", "new");
    assert_eq!(span.attributes["key"], "new");
    assert_eq!(span.attributes.len(), 1);
}

#[test]
fn dc_span_attributes_deterministic_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("zebra", "z")
        .with_attribute("alpha", "a")
        .with_attribute("middle", "m");
    let keys: Vec<_> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn dc_span_empty_key_and_value() {
    let span = TelemetrySpan::new("op").with_attribute("", "");
    assert_eq!(span.attributes[""], "");
}

#[test]
fn dc_span_clone_is_independent() {
    let span = TelemetrySpan::new("original").with_attribute("k", "v");
    let mut cloned = span.clone();
    cloned.attributes.insert("extra".into(), "val".into());
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(cloned.attributes.len(), 2);
}

#[test]
fn dc_span_serde_roundtrip() {
    let span = TelemetrySpan::new("run").with_attribute("backend", "mock");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
}

#[test]
fn dc_span_debug_format() {
    let dbg = format!("{:?}", TelemetrySpan::new("x"));
    assert!(dbg.contains("TelemetrySpan"));
}

#[test]
fn dc_span_many_attributes() {
    let mut span = TelemetrySpan::new("big");
    for i in 0..50 {
        span = span.with_attribute(format!("key_{i}"), format!("val_{i}"));
    }
    assert_eq!(span.attributes.len(), 50);
}

#[test]
fn dc_span_unicode_attribute() {
    let span = TelemetrySpan::new("OP_日本語").with_attribute("кey", "значение");
    assert_eq!(span.name, "OP_日本語");
    assert_eq!(span.attributes["кey"], "значение");
}

// ===========================================================================
// 12. TelemetrySpan emit and tracing integration
// ===========================================================================

#[test]
fn dc_span_emit_produces_output() {
    let (logs, _guard) = setup_tracing();
    let span = TelemetrySpan::new("emit_test").with_attribute("backend", "mock");
    span.emit();
    assert!(logs.contains("telemetry_span"), "logs: {}", logs.contents());
    assert!(logs.contains("emit_test"), "logs: {}", logs.contents());
}

#[test]
fn dc_span_emit_includes_attributes() {
    let (logs, _guard) = setup_tracing();
    TelemetrySpan::new("attr_check")
        .with_attribute("region", "us-east-1")
        .emit();
    assert!(logs.contains("attr_check"), "logs: {}", logs.contents());
}

#[test]
fn dc_nested_tracing_spans() {
    let (logs, _guard) = setup_tracing();
    let _outer = tracing::info_span!("parent_op").entered();
    tracing::info!("in parent");
    {
        let _inner = tracing::info_span!("child_op").entered();
        tracing::info!("in child");
    }
    assert!(logs.contains("parent_op"), "logs: {}", logs.contents());
    assert!(logs.contains("child_op"), "logs: {}", logs.contents());
}

#[test]
fn dc_deeply_nested_spans() {
    let (logs, _guard) = setup_tracing();
    let _l1 = tracing::info_span!("l1").entered();
    let _l2 = tracing::info_span!("l2").entered();
    let _l3 = tracing::info_span!("l3").entered();
    let _l4 = tracing::info_span!("l4").entered();
    tracing::info!("deep");
    assert!(logs.contains("l1"), "logs: {}", logs.contents());
    assert!(logs.contains("l4"), "logs: {}", logs.contents());
    assert!(logs.contains("deep"), "logs: {}", logs.contents());
}

#[test]
fn dc_tracing_info_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::info!("hello_info");
    assert!(logs.contains("hello_info"), "logs: {}", logs.contents());
}

#[test]
fn dc_tracing_debug_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::debug!("hello_debug");
    assert!(logs.contains("hello_debug"), "logs: {}", logs.contents());
}

#[test]
fn dc_tracing_warn_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::warn!("hello_warn");
    assert!(logs.contains("hello_warn"), "logs: {}", logs.contents());
}

#[test]
fn dc_tracing_error_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::error!("hello_error");
    assert!(logs.contains("hello_error"), "logs: {}", logs.contents());
}

#[test]
fn dc_tracing_trace_captured() {
    let (logs, _guard) = setup_tracing();
    tracing::trace!("hello_trace");
    assert!(logs.contains("hello_trace"), "logs: {}", logs.contents());
}

// ===========================================================================
// 13. Telemetry export (structured logging / JSON exporter)
// ===========================================================================

#[test]
fn dc_json_exporter_valid_output() {
    let c = MetricsCollector::new();
    c.record(mk("mock", 100, 0));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn dc_json_exporter_empty_summary() {
    let json = JsonExporter.export(&MetricsSummary::default()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
}

#[test]
fn dc_json_exporter_deterministic_backend_order() {
    let c = MetricsCollector::new();
    c.record(mk("zebra", 10, 0));
    c.record(mk("alpha", 20, 0));
    let json = JsonExporter.export(&c.summary()).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < zebra_pos);
}

#[test]
fn dc_json_exporter_pretty_printed() {
    let c = MetricsCollector::new();
    c.record(mk("x", 1, 0));
    let json = JsonExporter.export(&c.summary()).unwrap();
    // Pretty printing includes newlines and spaces
    assert!(json.contains('\n'));
}

#[test]
fn dc_json_exporter_roundtrip() {
    let c = MetricsCollector::new();
    c.record(mk("rt", 42, 1));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn dc_json_exporter_as_trait_object() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let result = exporter.export(&s);
    assert!(result.is_ok());
}

#[test]
fn dc_json_exporter_default() {
    let _e = JsonExporter;
}

#[test]
fn dc_json_exporter_debug() {
    let dbg = format!("{:?}", JsonExporter);
    assert!(dbg.contains("JsonExporter"));
}

// ===========================================================================
// 14. JSON tracing output format
// ===========================================================================

#[test]
fn dc_json_tracing_output() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(key = "val", "json event");
    let content = logs.contents();
    // JSON tracing emits JSON lines
    assert!(!content.is_empty(), "expected JSON output");
}

#[test]
fn dc_json_tracing_parseable() {
    let (logs, _guard) = setup_json_tracing();
    tracing::info!(metric = "test", "parseable_event");
    let content = logs.contents();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "line not valid JSON: {line}");
    }
}

// ===========================================================================
// 15. Concurrent metric updates
// ===========================================================================

#[test]
fn dc_concurrent_recording_10_threads() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(mk("thread", i * 10, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn dc_concurrent_recording_100_threads() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..100 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(mk("t", i, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 100);
}

#[test]
fn dc_concurrent_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(mk("pre", 10, 0));
    let mut handles = vec![];
    for _ in 0..5 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(mk("t", 20, 0));
            let _ = cc.summary();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 6);
}

#[test]
fn dc_concurrent_clone_record_read() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for _ in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(mk("x", 1, 0));
            let _ = cc.runs();
            let _ = cc.len();
            let _ = cc.is_empty();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
}

#[test]
fn dc_concurrent_clear_and_record() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    let h1 = thread::spawn(move || {
        for i in 0..50 {
            c2.record(mk("w", i, 0));
        }
    });
    let c3 = c.clone();
    let h2 = thread::spawn(move || {
        for _ in 0..10 {
            c3.clear();
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    // We can't predict the final length due to interleaving, but no panic
    let _ = c.len();
}

#[test]
fn dc_concurrent_summary_consistency() {
    let c = MetricsCollector::new();
    for i in 0..50 {
        c.record(mk("a", i + 1, 0));
    }
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || cc.summary()));
    }
    let summaries: Vec<MetricsSummary> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All summaries should be identical since no writes happen concurrently
    for s in &summaries {
        assert_eq!(s.count, 50);
    }
}

#[test]
fn dc_concurrent_multiple_collectors() {
    let c1 = MetricsCollector::new();
    let c2 = MetricsCollector::new();
    let cc1 = c1.clone();
    let cc2 = c2.clone();
    let h1 = thread::spawn(move || {
        for i in 0..25 {
            cc1.record(mk("c1", i, 0));
        }
    });
    let h2 = thread::spawn(move || {
        for i in 0..30 {
            cc2.record(mk("c2", i, 0));
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    assert_eq!(c1.len(), 25);
    assert_eq!(c2.len(), 30);
}

// ===========================================================================
// 16. Telemetry configuration (MetricsCollector lifecycle)
// ===========================================================================

#[test]
fn dc_collector_lifecycle_create_record_summarize_clear() {
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    c.record(mk("a", 10, 0));
    c.record(mk("b", 20, 1));
    assert_eq!(c.len(), 2);
    let s = c.summary();
    assert_eq!(s.count, 2);
    c.clear();
    assert!(c.is_empty());
}

#[test]
fn dc_collector_reuse_after_clear() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    c.clear();
    c.record(mk("b", 20, 0));
    c.record(mk("c", 30, 0));
    let s = c.summary();
    assert_eq!(s.count, 2);
    assert!(!s.backend_counts.contains_key("a"));
}

#[test]
fn dc_collector_summary_after_clear_and_refill() {
    let c = MetricsCollector::new();
    c.record(mk("a", 100, 0));
    c.clear();
    c.record(mk("b", 200, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 17. Telemetry filtering (tracing levels)
// ===========================================================================

#[test]
fn dc_filter_by_level_info_only() {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    tracing::info!("visible_info");
    tracing::debug!("invisible_debug");

    assert!(buf.contains("visible_info"), "logs: {}", buf.contents());
    assert!(
        !buf.contains("invisible_debug"),
        "debug should be filtered: {}",
        buf.contents()
    );
}

#[test]
fn dc_filter_by_level_warn_only() {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    tracing::warn!("visible_warn");
    tracing::info!("invisible_info");

    assert!(buf.contains("visible_warn"), "logs: {}", buf.contents());
    assert!(
        !buf.contains("invisible_info"),
        "info should be filtered: {}",
        buf.contents()
    );
}

#[test]
fn dc_filter_by_level_error_only() {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::ERROR)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    tracing::error!("visible_error");
    tracing::warn!("invisible_warn");

    assert!(buf.contains("visible_error"), "logs: {}", buf.contents());
    assert!(
        !buf.contains("invisible_warn"),
        "warn should be filtered: {}",
        buf.contents()
    );
}

// ===========================================================================
// 18. Telemetry context propagation (span nesting context)
// ===========================================================================

#[test]
fn dc_span_context_parent_child() {
    let (logs, _guard) = setup_tracing();
    let _parent = tracing::info_span!("ctx_parent").entered();
    {
        let _child = tracing::info_span!("ctx_child").entered();
        tracing::info!("ctx_msg");
    }
    let content = logs.contents();
    assert!(content.contains("ctx_parent"), "logs: {content}");
    assert!(content.contains("ctx_child"), "logs: {content}");
}

#[test]
fn dc_span_context_sibling_spans() {
    let (logs, _guard) = setup_tracing();
    let _parent = tracing::info_span!("ctx_root").entered();
    {
        let _a = tracing::info_span!("sibling_a").entered();
        tracing::info!("in_a");
    }
    {
        let _b = tracing::info_span!("sibling_b").entered();
        tracing::info!("in_b");
    }
    assert!(logs.contains("sibling_a"), "logs: {}", logs.contents());
    assert!(logs.contains("sibling_b"), "logs: {}", logs.contents());
}

#[test]
fn dc_span_context_event_fields() {
    let (logs, _guard) = setup_tracing();
    let _span = tracing::info_span!("field_span").entered();
    tracing::info!(run_id = "abc-123", duration_ms = 42, "run_complete");
    assert!(logs.contains("run_complete"), "logs: {}", logs.contents());
}

// ===========================================================================
// 19. Telemetry shutdown and flush (clear acts as flush)
// ===========================================================================

#[test]
fn dc_shutdown_clear_releases_memory() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(mk("bulk", i, 0));
    }
    assert_eq!(c.len(), 1000);
    c.clear();
    assert_eq!(c.len(), 0);
    assert!(c.runs().is_empty());
}

#[test]
fn dc_shutdown_summary_before_clear() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    c.record(mk("b", 20, 0));
    let s = c.summary();
    assert_eq!(s.count, 2);
    c.clear();
    assert_eq!(c.summary().count, 0);
}

#[test]
fn dc_shutdown_tracing_guard_drop() {
    // Verify that guard drop doesn't panic
    let (logs, guard) = setup_tracing();
    tracing::info!("before_drop");
    assert!(logs.contains("before_drop"));
    drop(guard);
    // After guard is dropped, the subscriber is no longer active, no panic
}

// ===========================================================================
// 20. Edge cases and boundary tests
// ===========================================================================

#[test]
fn dc_zero_duration_run() {
    let c = MetricsCollector::new();
    c.record(mk("a", 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms).abs() < f64::EPSILON);
}

#[test]
fn dc_max_u64_duration() {
    let c = MetricsCollector::new();
    c.record(RunMetrics {
        backend_name: "max".into(),
        dialect: "d".into(),
        duration_ms: u64::MAX,
        events_count: 0,
        tokens_in: 0,
        tokens_out: 0,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    });
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - u64::MAX as f64).abs() < 1.0);
}

#[test]
fn dc_single_duration_p50_p99_equal() {
    let c = MetricsCollector::new();
    c.record(mk("a", 77, 0));
    let s = c.summary();
    assert!((s.p50_duration_ms - s.p99_duration_ms).abs() < f64::EPSILON);
}

#[test]
fn dc_two_equal_durations() {
    let c = MetricsCollector::new();
    c.record(mk("a", 50, 0));
    c.record(mk("a", 50, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 50.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 50.0).abs() < f64::EPSILON);
}

#[test]
fn dc_summary_with_zero_tokens() {
    let c = MetricsCollector::new();
    c.record(mk_tokens("a", 0, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
}

#[test]
fn dc_span_no_attributes_serde() {
    let span = TelemetrySpan::new("bare");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "bare");
    assert!(span2.attributes.is_empty());
}

#[test]
fn dc_summary_count_matches_len() {
    let c = MetricsCollector::new();
    for i in 0..37 {
        c.record(mk("a", i, 0));
    }
    assert_eq!(c.summary().count, c.len());
}

#[test]
fn dc_empty_backend_name_in_counts() {
    let c = MetricsCollector::new();
    c.record(mk("", 1, 0));
    assert_eq!(c.summary().backend_counts[""], 1);
}

#[test]
fn dc_many_backends_in_summary() {
    let c = MetricsCollector::new();
    for i in 0..20 {
        c.record(mk(&format!("backend_{i:02}"), 1, 0));
    }
    assert_eq!(c.summary().backend_counts.len(), 20);
}

#[test]
fn dc_summary_deterministic_across_calls() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 0));
    c.record(mk("b", 20, 1));
    let s1 = c.summary();
    let s2 = c.summary();
    assert_eq!(s1, s2);
}

#[test]
fn dc_run_metrics_not_eq_to_default() {
    let m = mk("x", 1, 0);
    assert_ne!(m, RunMetrics::default());
}

#[test]
fn dc_span_deserialized_from_json_string() {
    let json = r#"{"name":"from_json","attributes":{"k":"v"}}"#;
    let span: TelemetrySpan = serde_json::from_str(json).unwrap();
    assert_eq!(span.name, "from_json");
    assert_eq!(span.attributes["k"], "v");
}

#[test]
fn dc_span_empty_attributes_in_json() {
    let json = r#"{"name":"empty","attributes":{}}"#;
    let span: TelemetrySpan = serde_json::from_str(json).unwrap();
    assert!(span.attributes.is_empty());
}

#[test]
fn dc_exporter_trait_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<JsonExporter>();
}

#[test]
fn dc_collector_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MetricsCollector>();
}

#[test]
fn dc_run_metrics_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RunMetrics>();
}

#[test]
fn dc_summary_partial_eq_reflexive() {
    let c = MetricsCollector::new();
    c.record(mk("a", 10, 1));
    let s = c.summary();
    assert_eq!(s, s);
}

#[test]
fn dc_summary_backend_counts_btreemap_type() {
    let s = MetricsSummary::default();
    let _map: &BTreeMap<String, usize> = &s.backend_counts;
}

#[test]
fn dc_multiple_exports_same_summary() {
    let c = MetricsCollector::new();
    c.record(mk("x", 50, 0));
    let s = c.summary();
    let j1 = JsonExporter.export(&s).unwrap();
    let j2 = JsonExporter.export(&s).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn dc_summary_after_many_records() {
    let c = MetricsCollector::new();
    for i in 1..=200 {
        c.record(mk("a", i, if i % 10 == 0 { 1 } else { 0 }));
    }
    let s = c.summary();
    assert_eq!(s.count, 200);
    assert!((s.mean_duration_ms - 100.5).abs() < f64::EPSILON);
    assert_eq!(s.backend_counts["a"], 200);
    // 20 errors / 200 runs = 0.1
    assert!((s.error_rate - 0.1).abs() < f64::EPSILON);
}

#[test]
fn dc_span_attribute_long_value() {
    let long = "x".repeat(10_000);
    let span = TelemetrySpan::new("long").with_attribute("data", long.clone());
    assert_eq!(span.attributes["data"], long);
}

#[test]
fn dc_run_metrics_long_backend_name() {
    let long = "b".repeat(10_000);
    let m = mk_full(&long, "d", 1, 0, 0, 0, 0, 0, 0);
    assert_eq!(m.backend_name.len(), 10_000);
}
