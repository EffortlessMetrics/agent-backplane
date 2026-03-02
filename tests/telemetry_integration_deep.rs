// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for abp-telemetry crate.

use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};
use std::sync::Arc;
use std::thread;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn make_run(
    backend: &str,
    dialect: &str,
    duration_ms: u64,
    events_count: u64,
    tokens_in: u64,
    tokens_out: u64,
    tool_calls_count: u64,
    errors_count: u64,
    emulations_applied: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: dialect.to_string(),
        duration_ms,
        events_count,
        tokens_in,
        tokens_out,
        tool_calls_count,
        errors_count,
        emulations_applied,
    }
}

fn quick_run(backend: &str, duration_ms: u64, errors: u64) -> RunMetrics {
    make_run(
        backend,
        "test_dialect",
        duration_ms,
        5,
        100,
        200,
        3,
        errors,
        0,
    )
}

/// Custom exporter that uppercases the JSON output.
struct UpperCaseExporter;

impl TelemetryExporter for UpperCaseExporter {
    fn export(&self, summary: &MetricsSummary) -> Result<String, String> {
        serde_json::to_string(summary)
            .map(|s| s.to_uppercase())
            .map_err(|e| e.to_string())
    }
}

/// Exporter that always fails.
struct FailingExporter;

impl TelemetryExporter for FailingExporter {
    fn export(&self, _summary: &MetricsSummary) -> Result<String, String> {
        Err("export failed on purpose".to_string())
    }
}

// ===========================================================================
// Category 1: RunMetrics construction & properties
// ===========================================================================

#[test]
fn run_metrics_default_is_all_zeroes() {
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
fn run_metrics_full_construction() {
    let m = make_run("openai", "gpt-4", 5000, 42, 1024, 2048, 7, 1, 3);
    assert_eq!(m.backend_name, "openai");
    assert_eq!(m.dialect, "gpt-4");
    assert_eq!(m.duration_ms, 5000);
    assert_eq!(m.events_count, 42);
    assert_eq!(m.tokens_in, 1024);
    assert_eq!(m.tokens_out, 2048);
    assert_eq!(m.tool_calls_count, 7);
    assert_eq!(m.errors_count, 1);
    assert_eq!(m.emulations_applied, 3);
}

#[test]
fn run_metrics_clone_independence() {
    let m1 = quick_run("a", 100, 0);
    let mut m2 = m1.clone();
    m2.backend_name = "b".to_string();
    m2.duration_ms = 999;
    assert_eq!(m1.backend_name, "a");
    assert_eq!(m1.duration_ms, 100);
    assert_eq!(m2.backend_name, "b");
    assert_eq!(m2.duration_ms, 999);
}

#[test]
fn run_metrics_partial_eq() {
    let a = quick_run("x", 50, 0);
    let b = quick_run("x", 50, 0);
    assert_eq!(a, b);
}

#[test]
fn run_metrics_not_equal_different_backend() {
    let a = quick_run("x", 50, 0);
    let b = quick_run("y", 50, 0);
    assert_ne!(a, b);
}

#[test]
fn run_metrics_not_equal_different_duration() {
    let a = quick_run("x", 50, 0);
    let b = quick_run("x", 51, 0);
    assert_ne!(a, b);
}

#[test]
fn run_metrics_debug_format() {
    let m = quick_run("dbg", 10, 0);
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("dbg"));
    assert!(dbg.contains("RunMetrics"));
}

#[test]
fn run_metrics_serde_roundtrip() {
    let m = make_run("claude", "sonnet", 999, 10, 500, 600, 2, 1, 1);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_serde_pretty_roundtrip() {
    let m = quick_run("pretty", 42, 0);
    let json = serde_json::to_string_pretty(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_json_field_names() {
    let m = quick_run("test", 100, 1);
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("\"backend_name\""));
    assert!(json.contains("\"dialect\""));
    assert!(json.contains("\"duration_ms\""));
    assert!(json.contains("\"events_count\""));
    assert!(json.contains("\"tokens_in\""));
    assert!(json.contains("\"tokens_out\""));
    assert!(json.contains("\"tool_calls_count\""));
    assert!(json.contains("\"errors_count\""));
    assert!(json.contains("\"emulations_applied\""));
}

#[test]
fn run_metrics_deserialize_from_json_object() {
    let json = r#"{
        "backend_name": "mock",
        "dialect": "v1",
        "duration_ms": 123,
        "events_count": 4,
        "tokens_in": 50,
        "tokens_out": 60,
        "tool_calls_count": 2,
        "errors_count": 0,
        "emulations_applied": 1
    }"#;
    let m: RunMetrics = serde_json::from_str(json).unwrap();
    assert_eq!(m.backend_name, "mock");
    assert_eq!(m.duration_ms, 123);
    assert_eq!(m.emulations_applied, 1);
}

#[test]
fn run_metrics_max_u64_values() {
    let m = RunMetrics {
        backend_name: "max".into(),
        dialect: "max".into(),
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
fn run_metrics_unicode_backend_name() {
    let m = RunMetrics {
        backend_name: "バックエンド".into(),
        dialect: "方言".into(),
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m.backend_name, m2.backend_name);
    assert_eq!(m.dialect, m2.dialect);
}

// ===========================================================================
// Category 2: MetricsCollector – basic operations
// ===========================================================================

#[test]
fn collector_new_is_empty() {
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_record_increments_len() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 0));
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
    c.record(quick_run("b", 20, 0));
    assert_eq!(c.len(), 2);
}

#[test]
fn collector_runs_returns_in_order() {
    let c = MetricsCollector::new();
    c.record(quick_run("first", 10, 0));
    c.record(quick_run("second", 20, 0));
    c.record(quick_run("third", 30, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].backend_name, "first");
    assert_eq!(runs[1].backend_name, "second");
    assert_eq!(runs[2].backend_name, "third");
}

#[test]
fn collector_runs_returns_clone() {
    let c = MetricsCollector::new();
    c.record(quick_run("test", 50, 0));
    let runs1 = c.runs();
    let runs2 = c.runs();
    assert_eq!(runs1, runs2);
    // Adding more after snapshot doesn't affect the snapshot
    c.record(quick_run("test2", 60, 0));
    assert_eq!(runs1.len(), 1);
    assert_eq!(c.runs().len(), 2);
}

#[test]
fn collector_clear_empties_all() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 0));
    c.record(quick_run("b", 20, 0));
    assert_eq!(c.len(), 2);
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert!(c.runs().is_empty());
}

#[test]
fn collector_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(quick_run("old", 10, 0));
    c.clear();
    c.record(quick_run("new", 20, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "new");
}

#[test]
fn collector_clone_shares_state() {
    let c1 = MetricsCollector::new();
    let c2 = c1.clone();
    c1.record(quick_run("from_c1", 10, 0));
    // Clone shares the inner Arc<Mutex<...>>, so c2 should see it
    assert_eq!(c2.len(), 1);
    assert_eq!(c2.runs()[0].backend_name, "from_c1");
}

#[test]
fn collector_debug_format() {
    let c = MetricsCollector::new();
    let dbg = format!("{:?}", c);
    assert!(dbg.contains("MetricsCollector"));
}

#[test]
fn collector_record_many() {
    let c = MetricsCollector::new();
    for i in 0..100 {
        c.record(quick_run("bulk", i, 0));
    }
    assert_eq!(c.len(), 100);
}

// ===========================================================================
// Category 3: MetricsSummary – aggregation math
// ===========================================================================

#[test]
fn summary_empty_collector_defaults() {
    let c = MetricsCollector::new();
    let s = c.summary();
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
fn summary_single_run() {
    let c = MetricsCollector::new();
    c.record(quick_run("solo", 42, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
    assert_eq!(s.total_tokens_in, 100);
    assert_eq!(s.total_tokens_out, 200);
    assert_eq!(s.error_rate, 0.0);
    assert_eq!(s.backend_counts["solo"], 1);
}

#[test]
fn summary_mean_duration_two_runs() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 100, 0));
    c.record(quick_run("a", 200, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_mean_duration_three_runs() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 100, 0));
    c.record(quick_run("a", 200, 0));
    c.record(quick_run("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_odd_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(quick_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_even_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(quick_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_two_equal() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 50, 0));
    c.record(quick_run("a", 50, 0));
    let s = c.summary();
    assert!((s.p50_duration_ms - 50.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p99_hundred_runs() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(quick_run("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn summary_p99_two_runs() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 0));
    c.record(quick_run("a", 1000, 0));
    let s = c.summary();
    // With 2 values: p99 rank = 0.99 * 1 = 0.99, lower=0, upper=1
    // result = 10 * 0.01 + 1000 * 0.99 = 0.1 + 990 = 990.1
    assert!(s.p99_duration_ms > 900.0);
}

#[test]
fn summary_total_tokens_accumulate() {
    let c = MetricsCollector::new();
    c.record(make_run("a", "d", 10, 1, 100, 200, 0, 0, 0));
    c.record(make_run("b", "d", 20, 1, 300, 400, 0, 0, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 400);
    assert_eq!(s.total_tokens_out, 600);
}

#[test]
fn summary_zero_tokens() {
    let c = MetricsCollector::new();
    c.record(make_run("a", "d", 10, 0, 0, 0, 0, 0, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
}

#[test]
fn summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 1));
    c.record(quick_run("a", 20, 1));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 0));
    c.record(quick_run("a", 20, 0));
    let s = c.summary();
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn summary_error_rate_partial() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 2));
    c.record(quick_run("a", 20, 0));
    c.record(quick_run("a", 30, 1));
    let s = c.summary();
    // 3 errors / 3 runs = 1.0
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_multi_errors_per_run() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 5));
    c.record(quick_run("a", 20, 0));
    // 5 errors / 2 runs = 2.5
    let s = c.summary();
    assert!((s.error_rate - 2.5).abs() < f64::EPSILON);
}

#[test]
fn summary_backend_counts_single() {
    let c = MetricsCollector::new();
    c.record(quick_run("alpha", 10, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["alpha"], 1);
}

#[test]
fn summary_backend_counts_multiple_backends() {
    let c = MetricsCollector::new();
    c.record(quick_run("alpha", 10, 0));
    c.record(quick_run("beta", 20, 0));
    c.record(quick_run("alpha", 30, 0));
    c.record(quick_run("gamma", 40, 0));
    c.record(quick_run("beta", 50, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 2);
    assert_eq!(s.backend_counts["gamma"], 1);
}

#[test]
fn summary_backend_counts_deterministic_order() {
    let c = MetricsCollector::new();
    c.record(quick_run("zebra", 10, 0));
    c.record(quick_run("alpha", 20, 0));
    c.record(quick_run("middle", 30, 0));
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn summary_after_clear_is_empty() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 10, 0));
    c.clear();
    let s = c.summary();
    assert_eq!(s, MetricsSummary::default());
}

#[test]
fn summary_all_same_duration() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(quick_run("same", 42, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_unsorted_durations_sorted_internally() {
    let c = MetricsCollector::new();
    // Record in non-sorted order; summary should sort internally
    for d in [50, 10, 40, 20, 30] {
        c.record(quick_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
    assert!((s.mean_duration_ms - 30.0).abs() < f64::EPSILON);
}

// ===========================================================================
// Category 4: MetricsSummary – serde & properties
// ===========================================================================

#[test]
fn metrics_summary_default_values() {
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
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(quick_run("alpha", 50, 1));
    c.record(quick_run("beta", 100, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_json_field_names() {
    let s = MetricsSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"count\""));
    assert!(json.contains("\"mean_duration_ms\""));
    assert!(json.contains("\"p50_duration_ms\""));
    assert!(json.contains("\"p99_duration_ms\""));
    assert!(json.contains("\"total_tokens_in\""));
    assert!(json.contains("\"total_tokens_out\""));
    assert!(json.contains("\"error_rate\""));
    assert!(json.contains("\"backend_counts\""));
}

#[test]
fn metrics_summary_clone_independence() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 100, 0));
    let s1 = c.summary();
    let s2 = s1.clone();
    assert_eq!(s1, s2);
}

#[test]
fn metrics_summary_debug_format() {
    let s = MetricsSummary::default();
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("MetricsSummary"));
}

#[test]
fn metrics_summary_partial_eq_different() {
    let mut s1 = MetricsSummary::default();
    let mut s2 = MetricsSummary::default();
    s1.count = 5;
    s2.count = 10;
    assert_ne!(s1, s2);
}

// ===========================================================================
// Category 5: TelemetrySpan – creation and attributes
// ===========================================================================

#[test]
fn span_new_basic() {
    let span = TelemetrySpan::new("my_operation");
    assert_eq!(span.name, "my_operation");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_new_from_string() {
    let name = String::from("owned_name");
    let span = TelemetrySpan::new(name);
    assert_eq!(span.name, "owned_name");
}

#[test]
fn span_with_single_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("key", "value");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "value");
}

#[test]
fn span_with_multiple_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["a"], "1");
    assert_eq!(span.attributes["b"], "2");
    assert_eq!(span.attributes["c"], "3");
}

#[test]
fn span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "old")
        .with_attribute("key", "new");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "new");
}

#[test]
fn span_attributes_deterministic_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("zebra", "z")
        .with_attribute("alpha", "a")
        .with_attribute("middle", "m");
    let keys: Vec<&String> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn span_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_unicode_name_and_attributes() {
    let span = TelemetrySpan::new("操作").with_attribute("キー", "値");
    assert_eq!(span.name, "操作");
    assert_eq!(span.attributes["キー"], "値");
}

#[test]
fn span_clone_independence() {
    let span1 = TelemetrySpan::new("op").with_attribute("k", "v");
    let mut span2 = span1.clone();
    span2.name = "op2".to_string();
    span2.attributes.insert("k2".into(), "v2".into());
    assert_eq!(span1.name, "op");
    assert_eq!(span1.attributes.len(), 1);
    assert_eq!(span2.name, "op2");
    assert_eq!(span2.attributes.len(), 2);
}

#[test]
fn span_serde_roundtrip() {
    let span = TelemetrySpan::new("run")
        .with_attribute("backend", "mock")
        .with_attribute("dialect", "test");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["dialect"], "test");
}

#[test]
fn span_serde_json_field_names() {
    let span = TelemetrySpan::new("test").with_attribute("k", "v");
    let json = serde_json::to_string(&span).unwrap();
    assert!(json.contains("\"name\""));
    assert!(json.contains("\"attributes\""));
}

#[test]
fn span_debug_format() {
    let span = TelemetrySpan::new("debug_test");
    let dbg = format!("{:?}", span);
    assert!(dbg.contains("TelemetrySpan"));
    assert!(dbg.contains("debug_test"));
}

#[test]
fn span_emit_does_not_panic() {
    // Just ensure emit() doesn't panic when no subscriber is installed.
    let span = TelemetrySpan::new("safe_emit").with_attribute("k", "v");
    span.emit();
}

#[test]
fn span_many_attributes() {
    let mut span = TelemetrySpan::new("big");
    for i in 0..50 {
        span = span.with_attribute(format!("key_{i}"), format!("val_{i}"));
    }
    assert_eq!(span.attributes.len(), 50);
}

// ===========================================================================
// Category 6: TelemetryExporter trait & JsonExporter
// ===========================================================================

#[test]
fn json_exporter_default() {
    let _e = JsonExporter;
}

#[test]
fn json_exporter_debug() {
    let e = JsonExporter;
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("JsonExporter"));
}

#[test]
fn json_exporter_empty_summary() {
    let e = JsonExporter;
    let s = MetricsSummary::default();
    let json = e.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
}

#[test]
fn json_exporter_with_data() {
    let c = MetricsCollector::new();
    c.record(quick_run("mock", 100, 0));
    c.record(quick_run("mock", 200, 1));
    let s = c.summary();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 2);
    assert!(parsed["mean_duration_ms"].as_f64().unwrap() > 0.0);
    assert_eq!(parsed["total_tokens_in"], 200);
    assert_eq!(parsed["total_tokens_out"], 400);
}

#[test]
fn json_exporter_pretty_printed() {
    let s = MetricsSummary::default();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    // Pretty-printed JSON should contain newlines
    assert!(json.contains('\n'));
}

#[test]
fn json_exporter_backend_counts_alphabetical() {
    let c = MetricsCollector::new();
    c.record(quick_run("zzz", 10, 0));
    c.record(quick_run("aaa", 20, 0));
    let s = c.summary();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    let aaa_pos = json.find("\"aaa\"").unwrap();
    let zzz_pos = json.find("\"zzz\"").unwrap();
    assert!(aaa_pos < zzz_pos);
}

#[test]
fn json_exporter_valid_json_output() {
    let c = MetricsCollector::new();
    for i in 0..5 {
        c.record(quick_run(&format!("backend_{i}"), i * 10 + 1, i % 2));
    }
    let s = c.summary();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    // Must parse as valid JSON
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

#[test]
fn custom_exporter_uppercase() {
    let s = MetricsSummary::default();
    let e = UpperCaseExporter;
    let result = e.export(&s).unwrap();
    // All alpha chars should be uppercase
    assert!(!result.chars().any(|c| c.is_ascii_lowercase()));
}

#[test]
fn custom_exporter_failing() {
    let s = MetricsSummary::default();
    let e = FailingExporter;
    let result = e.export(&s);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "export failed on purpose");
}

#[test]
fn exporter_trait_object_json() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let result = exporter.export(&s);
    assert!(result.is_ok());
}

#[test]
fn exporter_trait_object_custom() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(UpperCaseExporter);
    let s = MetricsSummary::default();
    let result = exporter.export(&s);
    assert!(result.is_ok());
}

#[test]
fn exporter_trait_object_failing() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(FailingExporter);
    let s = MetricsSummary::default();
    assert!(exporter.export(&s).is_err());
}

#[test]
fn exporter_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<JsonExporter>();
}

// ===========================================================================
// Category 7: Thread safety
// ===========================================================================

#[test]
fn concurrent_record_10_threads() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(quick_run("thread", i * 10, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn concurrent_record_100_threads() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..100 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(quick_run("bulk", i, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 100);
}

#[test]
fn concurrent_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(quick_run("pre", 10, 0));
    let mut handles = vec![];
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(quick_run("t", i * 10, 0));
            let _ = cc.summary();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 11);
}

#[test]
fn concurrent_clear_while_recording() {
    let c = MetricsCollector::new();
    let c1 = c.clone();
    let c2 = c.clone();
    let h1 = thread::spawn(move || {
        for i in 0..50 {
            c1.record(quick_run("writer", i, 0));
        }
    });
    let h2 = thread::spawn(move || {
        for _ in 0..5 {
            c2.clear();
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    // Length is non-deterministic but should not panic
    let _ = c.len();
}

#[test]
fn arc_wrapped_collector() {
    let c = Arc::new(MetricsCollector::new());
    let mut handles = vec![];
    for i in 0..10 {
        let cc = Arc::clone(&c);
        handles.push(thread::spawn(move || {
            cc.record(quick_run("arc", i, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn concurrent_runs_snapshot() {
    let c = MetricsCollector::new();
    for i in 0..20 {
        c.record(quick_run("init", i, 0));
    }
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            let runs = cc.runs();
            assert!(runs.len() >= 20);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}

// ===========================================================================
// Category 8: Edge cases
// ===========================================================================

#[test]
fn zero_duration_run() {
    let c = MetricsCollector::new();
    c.record(quick_run("zero", 0, 0));
    let s = c.summary();
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.p50_duration_ms, 0.0);
}

#[test]
fn very_large_duration() {
    let c = MetricsCollector::new();
    c.record(quick_run("big", u64::MAX, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!(s.mean_duration_ms > 0.0);
}

#[test]
fn empty_backend_name() {
    let c = MetricsCollector::new();
    c.record(quick_run("", 10, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts[""], 1);
}

#[test]
fn backend_name_with_special_chars() {
    let c = MetricsCollector::new();
    c.record(quick_run("back/end-1.0@test", 10, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["back/end-1.0@test"], 1);
}

#[test]
fn summary_recomputed_after_more_records() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 100, 0));
    let s1 = c.summary();
    assert_eq!(s1.count, 1);

    c.record(quick_run("b", 200, 0));
    let s2 = c.summary();
    assert_eq!(s2.count, 2);
    assert!(s2.mean_duration_ms > s1.mean_duration_ms);
}

#[test]
fn summary_idempotent() {
    let c = MetricsCollector::new();
    c.record(quick_run("a", 100, 0));
    let s1 = c.summary();
    let s2 = c.summary();
    assert_eq!(s1, s2);
}

#[test]
fn run_metrics_with_all_zero_tokens() {
    let m = make_run("z", "d", 100, 0, 0, 0, 0, 0, 0);
    assert_eq!(m.tokens_in, 0);
    assert_eq!(m.tokens_out, 0);
}

#[test]
fn span_empty_attribute_key_and_value() {
    let span = TelemetrySpan::new("op").with_attribute("", "");
    assert_eq!(span.attributes[""], "");
}

#[test]
fn span_long_name() {
    let long_name = "a".repeat(10_000);
    let span = TelemetrySpan::new(long_name.clone());
    assert_eq!(span.name, long_name);
}

#[test]
fn span_long_attribute_value() {
    let long_val = "x".repeat(10_000);
    let span = TelemetrySpan::new("op").with_attribute("big", long_val.clone());
    assert_eq!(span.attributes["big"], long_val);
}

#[test]
fn collector_record_default_run_metrics() {
    let c = MetricsCollector::new();
    c.record(RunMetrics::default());
    assert_eq!(c.len(), 1);
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, 0.0);
}

#[test]
fn exporter_with_complex_summary() {
    let c = MetricsCollector::new();
    for i in 0..50 {
        let backend = format!("backend_{}", i % 5);
        c.record(make_run(
            &backend,
            "test",
            i * 10 + 1,
            i,
            i * 100,
            i * 200,
            i % 3,
            i % 7,
            i % 2,
        ));
    }
    let s = c.summary();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 50);
    assert_eq!(s.backend_counts.len(), 5);
}

#[test]
fn multiple_clear_cycles() {
    let c = MetricsCollector::new();
    for cycle in 0..5 {
        for i in 0..10 {
            c.record(quick_run(&format!("c{cycle}"), i, 0));
        }
        assert_eq!(c.len(), 10);
        let s = c.summary();
        assert_eq!(s.count, 10);
        c.clear();
        assert!(c.is_empty());
    }
}

#[test]
fn summary_single_backend_many_runs() {
    let c = MetricsCollector::new();
    for i in 1..=1000 {
        c.record(quick_run("single", i, 0));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["single"], 1000);
    // Mean of 1..=1000 is 500.5
    assert!((s.mean_duration_ms - 500.5).abs() < f64::EPSILON);
}

#[test]
fn summary_many_backends() {
    let c = MetricsCollector::new();
    for i in 0..100 {
        c.record(quick_run(&format!("backend_{i:03}"), 10, 0));
    }
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 100);
    // BTreeMap keys should be sorted
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys);
}

#[test]
fn json_exporter_roundtrip_through_export() {
    let c = MetricsCollector::new();
    c.record(quick_run("rt", 42, 1));
    let s = c.summary();
    let e = JsonExporter;
    let json = e.export(&s).unwrap();
    let deserialized: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, deserialized);
}

#[test]
fn span_deserialize_from_json() {
    let json = r#"{"name":"deserialized","attributes":{"k1":"v1","k2":"v2"}}"#;
    let span: TelemetrySpan = serde_json::from_str(json).unwrap();
    assert_eq!(span.name, "deserialized");
    assert_eq!(span.attributes.len(), 2);
    assert_eq!(span.attributes["k1"], "v1");
}

#[test]
fn span_empty_attributes_json() {
    let json = r#"{"name":"minimal","attributes":{}}"#;
    let span: TelemetrySpan = serde_json::from_str(json).unwrap();
    assert_eq!(span.name, "minimal");
    assert!(span.attributes.is_empty());
}

#[test]
fn collector_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<MetricsCollector>();
    assert_sync::<MetricsCollector>();
}

#[test]
fn run_metrics_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<RunMetrics>();
    assert_sync::<RunMetrics>();
}

#[test]
fn span_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<TelemetrySpan>();
    assert_sync::<TelemetrySpan>();
}

#[test]
fn metrics_summary_is_send_and_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<MetricsSummary>();
    assert_sync::<MetricsSummary>();
}

#[test]
fn btreemap_in_summary_is_deterministic_json() {
    let c = MetricsCollector::new();
    c.record(quick_run("z", 10, 0));
    c.record(quick_run("a", 20, 0));
    c.record(quick_run("m", 30, 0));
    let s = c.summary();
    let json1 = serde_json::to_string(&s).unwrap();
    let json2 = serde_json::to_string(&s).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn run_metrics_deterministic_serialization() {
    let m = quick_run("det", 42, 0);
    let json1 = serde_json::to_string(&m).unwrap();
    let json2 = serde_json::to_string(&m).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn span_deterministic_serialization() {
    let span = TelemetrySpan::new("det")
        .with_attribute("z", "1")
        .with_attribute("a", "2");
    let json1 = serde_json::to_string(&span).unwrap();
    let json2 = serde_json::to_string(&span).unwrap();
    assert_eq!(json1, json2);
}
