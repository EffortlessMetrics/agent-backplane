#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive telemetry and metrics tests for `abp-telemetry`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use abp_telemetry::hooks::{
    on_error, on_request_complete, on_request_start, ErrorClassification, RequestOutcome,
};
use abp_telemetry::metrics::{
    ActiveRequestGauge, ErrorCounter, RequestCounter, RequestKey, TokenAccumulator,
};
use abp_telemetry::pipeline::{
    TelemetryCollector, TelemetryEvent, TelemetryEventType, TelemetryFilter, TelemetrySummary,
};
use abp_telemetry::spans::{backend_span, event_span, request_span};
use abp_telemetry::{
    CostEstimator, ExportFormat, JsonExporter, LatencyHistogram, MetricsCollector, MetricsExporter,
    MetricsSummary, ModelPricing, RunMetrics, RunSummary, TelemetryExporter, TelemetrySpan,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample(backend: &str, duration: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: "test".into(),
        duration_ms: duration,
        events_count: 5,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 3,
        errors_count: errors,
        emulations_applied: 1,
    }
}

fn sample_full(
    backend: &str,
    duration: u64,
    tokens_in: u64,
    tokens_out: u64,
    errors: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: "test".into(),
        duration_ms: duration,
        events_count: 1,
        tokens_in,
        tokens_out,
        tool_calls_count: 0,
        errors_count: errors,
        emulations_applied: 0,
    }
}

fn make_event(
    event_type: TelemetryEventType,
    run_id: Option<&str>,
    backend: Option<&str>,
    duration_ms: Option<u64>,
) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type,
        run_id: run_id.map(String::from),
        backend: backend.map(String::from),
        metadata: BTreeMap::new(),
        duration_ms,
    }
}

// ===========================================================================
// 1. MetricsCollector — basic operations
// ===========================================================================

#[test]
fn mc_new_is_empty() {
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn mc_default_is_empty() {
    let c = MetricsCollector::default();
    assert!(c.is_empty());
}

#[test]
fn mc_record_increments_len() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
}

#[test]
fn mc_record_multiple() {
    let c = MetricsCollector::new();
    for i in 0..5 {
        c.record(sample("b", i * 10, 0));
    }
    assert_eq!(c.len(), 5);
}

#[test]
fn mc_runs_returns_copies() {
    let c = MetricsCollector::new();
    c.record(sample("x", 1, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].backend_name, "x");
}

#[test]
fn mc_clear_empties() {
    let c = MetricsCollector::new();
    c.record(sample("y", 2, 0));
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn mc_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(sample("a", 1, 0));
    c.clear();
    c.record(sample("b", 2, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

// ===========================================================================
// 2. MetricsCollector — summary
// ===========================================================================

#[test]
fn mc_summary_empty() {
    let c = MetricsCollector::new();
    let s = c.summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
    assert_eq!(s.error_rate, 0.0);
    assert!(s.backend_counts.is_empty());
}

#[test]
fn mc_summary_single_run() {
    let c = MetricsCollector::new();
    c.record(sample("mock", 42, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
    assert_eq!(s.backend_counts["mock"], 1);
}

#[test]
fn mc_summary_mean_duration() {
    let c = MetricsCollector::new();
    c.record(sample("a", 100, 0));
    c.record(sample("a", 200, 0));
    c.record(sample("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn mc_summary_p50_odd() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn mc_summary_p50_even() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn mc_summary_p99_large() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(sample("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn mc_summary_total_tokens() {
    let c = MetricsCollector::new();
    c.record(sample_full("a", 10, 50, 100, 0));
    c.record(sample_full("b", 20, 150, 200, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 200);
    assert_eq!(s.total_tokens_out, 300);
}

#[test]
fn mc_summary_error_rate_zero() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 0));
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn mc_summary_error_rate_nonzero() {
    let c = MetricsCollector::new();
    c.record(sample("a", 10, 1));
    c.record(sample("a", 20, 0));
    c.record(sample("a", 30, 2));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn mc_summary_backend_counts() {
    let c = MetricsCollector::new();
    c.record(sample("alpha", 10, 0));
    c.record(sample("beta", 20, 0));
    c.record(sample("alpha", 30, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 1);
}

// ===========================================================================
// 3. RunMetrics serde
// ===========================================================================

#[test]
fn run_metrics_default() {
    let m = RunMetrics::default();
    assert_eq!(m.backend_name, "");
    assert_eq!(m.duration_ms, 0);
    assert_eq!(m.tokens_in, 0);
    assert_eq!(m.tokens_out, 0);
    assert_eq!(m.errors_count, 0);
    assert_eq!(m.emulations_applied, 0);
}

#[test]
fn run_metrics_serde_roundtrip() {
    let m = sample("serde_test", 999, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_clone_eq() {
    let m = sample("clone", 50, 1);
    let m2 = m.clone();
    assert_eq!(m, m2);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(sample("a", 50, 1));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_default() {
    let s = MetricsSummary::default();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert!(s.backend_counts.is_empty());
}

// ===========================================================================
// 4. TelemetrySpan
// ===========================================================================

#[test]
fn span_new_name() {
    let span = TelemetrySpan::new("my_op");
    assert_eq!(span.name, "my_op");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_with_one_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("k", "v");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["k"], "v");
}

#[test]
fn span_with_multiple_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("a", "1")
        .with_attribute("b", "2")
        .with_attribute("c", "3");
    assert_eq!(span.attributes.len(), 3);
}

#[test]
fn span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("k", "old")
        .with_attribute("k", "new");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["k"], "new");
}

#[test]
fn span_serde_roundtrip() {
    let span = TelemetrySpan::new("run").with_attribute("backend", "mock");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
}

#[test]
fn span_emit_does_not_panic() {
    let span = TelemetrySpan::new("test").with_attribute("key", "val");
    span.emit();
}

#[test]
fn span_attributes_deterministic_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z", "last")
        .with_attribute("a", "first");
    let keys: Vec<&String> = span.attributes.keys().collect();
    assert_eq!(keys[0], "a");
    assert_eq!(keys[1], "z");
}

// ===========================================================================
// 5. JsonExporter / TelemetryExporter trait
// ===========================================================================

#[test]
fn json_exporter_valid_json() {
    let c = MetricsCollector::new();
    c.record(sample("mock", 100, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
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
fn json_exporter_deterministic_backend_keys() {
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
// 6. MetricsExporter multi-format
// ===========================================================================

#[test]
fn exporter_json_format() {
    let c = MetricsCollector::new();
    c.record(sample("a", 50, 0));
    let s = c.summary();
    let json = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn exporter_csv_format() {
    let c = MetricsCollector::new();
    c.record(sample("a", 50, 0));
    let s = c.summary();
    let csv = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(csv.contains("count,"));
    assert!(csv.contains("mean_duration_ms"));
}

#[test]
fn exporter_structured_format() {
    let c = MetricsCollector::new();
    c.record(sample("a", 50, 0));
    let s = c.summary();
    let structured = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(structured.contains("count=1"));
    assert!(structured.contains("mean_duration_ms="));
}

#[test]
fn exporter_csv_runs() {
    let runs = vec![sample("mock", 100, 0), sample("sidecar", 200, 1)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    assert!(csv.contains("backend_name,"));
    assert!(csv.contains("mock"));
    assert!(csv.contains("sidecar"));
}

#[test]
fn exporter_structured_contains_all_fields() {
    let c = MetricsCollector::new();
    c.record(sample("test_be", 50, 0));
    let s = c.summary();
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count="));
    assert!(out.contains("p50_duration_ms="));
    assert!(out.contains("p99_duration_ms="));
    assert!(out.contains("total_tokens_in="));
    assert!(out.contains("total_tokens_out="));
    assert!(out.contains("error_rate="));
    assert!(out.contains("backend.test_be="));
}

#[test]
fn export_format_serde_roundtrip() {
    let fmt = ExportFormat::Json;
    let json = serde_json::to_string(&fmt).unwrap();
    let fmt2: ExportFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(fmt, fmt2);
}

#[test]
fn export_format_all_variants() {
    assert_eq!(ExportFormat::Json, ExportFormat::Json);
    assert_eq!(ExportFormat::Csv, ExportFormat::Csv);
    assert_eq!(ExportFormat::Structured, ExportFormat::Structured);
    assert_ne!(ExportFormat::Json, ExportFormat::Csv);
}

// ===========================================================================
// 7. LatencyHistogram
// ===========================================================================

#[test]
fn histogram_new_is_empty() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.count(), 0);
}

#[test]
fn histogram_record_increments_count() {
    let mut h = LatencyHistogram::new();
    h.record(1.0);
    assert_eq!(h.count(), 1);
    assert!(!h.is_empty());
}

#[test]
fn histogram_min_max_single() {
    let mut h = LatencyHistogram::new();
    h.record(42.0);
    assert_eq!(h.min(), Some(42.0));
    assert_eq!(h.max(), Some(42.0));
}

#[test]
fn histogram_min_max_multiple() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(50.0);
    h.record(30.0);
    assert_eq!(h.min(), Some(10.0));
    assert_eq!(h.max(), Some(50.0));
}

#[test]
fn histogram_mean_empty() {
    let h = LatencyHistogram::new();
    assert_eq!(h.mean(), 0.0);
}

#[test]
fn histogram_mean_single() {
    let mut h = LatencyHistogram::new();
    h.record(7.0);
    assert!((h.mean() - 7.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_mean_multiple() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);
    assert!((h.mean() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_p50_single() {
    let mut h = LatencyHistogram::new();
    h.record(99.0);
    assert!((h.p50() - 99.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_p50_three_values() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);
    assert!((h.p50() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_p95() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    let p95 = h.p95();
    assert!(p95 > 94.0 && p95 <= 96.0);
}

#[test]
fn histogram_p99() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    let p99 = h.p99();
    assert!(p99 > 98.0 && p99 <= 100.0);
}

#[test]
fn histogram_percentile_empty() {
    let h = LatencyHistogram::new();
    assert_eq!(h.percentile(50.0), 0.0);
}

#[test]
fn histogram_percentile_0() {
    let mut h = LatencyHistogram::new();
    h.record(5.0);
    h.record(10.0);
    h.record(15.0);
    assert!((h.percentile(0.0) - 5.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_percentile_100() {
    let mut h = LatencyHistogram::new();
    h.record(5.0);
    h.record(10.0);
    h.record(15.0);
    assert!((h.percentile(100.0) - 15.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_merge() {
    let mut a = LatencyHistogram::new();
    a.record(10.0);
    a.record(20.0);
    let mut b = LatencyHistogram::new();
    b.record(30.0);
    a.merge(&b);
    assert_eq!(a.count(), 3);
    assert_eq!(a.max(), Some(30.0));
}

#[test]
fn histogram_merge_empty_into_non_empty() {
    let mut a = LatencyHistogram::new();
    a.record(5.0);
    let b = LatencyHistogram::new();
    a.merge(&b);
    assert_eq!(a.count(), 1);
}

#[test]
fn histogram_merge_non_empty_into_empty() {
    let mut a = LatencyHistogram::new();
    let mut b = LatencyHistogram::new();
    b.record(42.0);
    a.merge(&b);
    assert_eq!(a.count(), 1);
    assert_eq!(a.min(), Some(42.0));
}

#[test]
fn histogram_buckets_basic() {
    let mut h = LatencyHistogram::new();
    for v in [1.0, 5.0, 15.0, 50.0, 150.0] {
        h.record(v);
    }
    let buckets = h.buckets(&[10.0, 100.0]);
    // [0, 10): 1.0, 5.0 => 2
    // [10, 100): 15.0, 50.0 => 2
    // [100, inf): 150.0 => 1
    assert_eq!(buckets, vec![2, 2, 1]);
}

#[test]
fn histogram_buckets_all_in_first() {
    let mut h = LatencyHistogram::new();
    h.record(1.0);
    h.record(2.0);
    let buckets = h.buckets(&[100.0]);
    assert_eq!(buckets, vec![2, 0]);
}

#[test]
fn histogram_buckets_all_in_last() {
    let mut h = LatencyHistogram::new();
    h.record(500.0);
    h.record(600.0);
    let buckets = h.buckets(&[10.0, 100.0]);
    assert_eq!(buckets, vec![0, 0, 2]);
}

#[test]
fn histogram_serde_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(1.0);
    h.record(2.0);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
}

#[test]
fn histogram_min_max_none_when_empty() {
    let h = LatencyHistogram::new();
    assert!(h.min().is_none());
    assert!(h.max().is_none());
}

// ===========================================================================
// 8. RunSummary
// ===========================================================================

#[test]
fn run_summary_new_defaults() {
    let s = RunSummary::new();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.error_count, 0);
    assert_eq!(s.warning_count, 0);
    assert_eq!(s.tool_call_count, 0);
    assert!(s.event_counts.is_empty());
}

#[test]
fn run_summary_record_error() {
    let mut s = RunSummary::new();
    s.record_event("error");
    assert_eq!(s.error_count, 1);
    assert_eq!(s.total_events, 1);
    assert!(s.has_errors());
}

#[test]
fn run_summary_record_warning() {
    let mut s = RunSummary::new();
    s.record_event("warning");
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.total_events, 1);
}

#[test]
fn run_summary_record_tool_call() {
    let mut s = RunSummary::new();
    s.record_event("tool_call");
    assert_eq!(s.tool_call_count, 1);
}

#[test]
fn run_summary_record_unknown_kind() {
    let mut s = RunSummary::new();
    s.record_event("custom");
    assert_eq!(s.total_events, 1);
    assert_eq!(s.error_count, 0);
    assert_eq!(s.event_counts["custom"], 1);
}

#[test]
fn run_summary_has_errors_false() {
    let s = RunSummary::new();
    assert!(!s.has_errors());
}

#[test]
fn run_summary_error_rate_no_events() {
    let s = RunSummary::new();
    assert_eq!(s.error_rate(), 0.0);
}

#[test]
fn run_summary_error_rate_with_events() {
    let mut s = RunSummary::new();
    s.record_event("error");
    s.record_event("tool_call");
    s.record_event("tool_call");
    s.record_event("tool_call");
    assert!((s.error_rate() - 0.25).abs() < f64::EPSILON);
}

#[test]
fn run_summary_set_duration() {
    let mut s = RunSummary::new();
    s.set_duration(500);
    assert_eq!(s.total_duration_ms, 500);
}

#[test]
fn run_summary_merge() {
    let mut a = RunSummary::new();
    a.record_event("error");
    a.set_duration(100);
    let mut b = RunSummary::new();
    b.record_event("tool_call");
    b.record_event("tool_call");
    b.set_duration(200);
    a.merge(&b);
    assert_eq!(a.total_events, 3);
    assert_eq!(a.error_count, 1);
    assert_eq!(a.tool_call_count, 2);
    assert_eq!(a.total_duration_ms, 300);
}

#[test]
fn run_summary_merge_event_counts() {
    let mut a = RunSummary::new();
    a.record_event("error");
    let mut b = RunSummary::new();
    b.record_event("error");
    b.record_event("error");
    a.merge(&b);
    assert_eq!(a.event_counts["error"], 3);
}

#[test]
fn run_summary_from_events() {
    let s = RunSummary::from_events(&["error", "tool_call", "tool_call", "warning"], 1000);
    assert_eq!(s.total_events, 4);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.tool_call_count, 2);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.total_duration_ms, 1000);
}

#[test]
fn run_summary_from_events_empty() {
    let s = RunSummary::from_events(&[], 0);
    assert_eq!(s.total_events, 0);
    assert_eq!(s.total_duration_ms, 0);
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["error", "tool_call"], 250);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

// ===========================================================================
// 9. CostEstimator
// ===========================================================================

#[test]
fn cost_estimator_new_empty() {
    let e = CostEstimator::new();
    assert!(e.models().is_empty());
}

#[test]
fn cost_estimator_set_and_get_pricing() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    assert!(e.get_pricing("gpt-4").is_some());
    assert!(e.get_pricing("unknown").is_none());
}

#[test]
fn cost_estimator_estimate_known() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let cost = e.estimate("gpt-4", 100, 50).unwrap();
    let expected = 100.0 * 0.001 + 50.0 * 0.002;
    assert!((cost - expected).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_estimate_unknown() {
    let e = CostEstimator::new();
    assert!(e.estimate("gpt-5", 100, 50).is_none());
}

#[test]
fn cost_estimator_estimate_total() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "a",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    e.set_pricing(
        "b",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    let total = e.estimate_total(&[("a", 100, 100), ("b", 10, 10)]);
    let expected = (100.0 * 0.001 + 100.0 * 0.002) + (10.0 * 0.01 + 10.0 * 0.02);
    assert!((total - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_estimate_total_skips_unknown() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "a",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.001,
        },
    );
    let total = e.estimate_total(&[("a", 100, 100), ("unknown", 1000, 1000)]);
    let expected = 100.0 * 0.001 + 100.0 * 0.001;
    assert!((total - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_models_list() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "z_model",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    e.set_pricing(
        "a_model",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    let models = e.models();
    assert_eq!(models.len(), 2);
    // BTreeMap ensures alphabetical order
    assert_eq!(models[0], "a_model");
    assert_eq!(models[1], "z_model");
}

#[test]
fn cost_estimator_overwrite_pricing() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.001,
        },
    );
    e.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.01,
        },
    );
    let p = e.get_pricing("m").unwrap();
    assert!((p.input_cost_per_token - 0.01).abs() < f64::EPSILON);
}

#[test]
fn model_pricing_serde_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.00003,
        output_cost_per_token: 0.00006,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

// ===========================================================================
// 10. RequestCounter (metrics module)
// ===========================================================================

#[test]
fn request_counter_empty() {
    let c = RequestCounter::new();
    assert_eq!(c.total(), 0);
    assert_eq!(c.get("a", "b", "c"), 0);
}

#[test]
fn request_counter_increment_and_get() {
    let c = RequestCounter::new();
    c.increment("mock", "openai", "success");
    c.increment("mock", "openai", "success");
    c.increment("mock", "openai", "error");
    assert_eq!(c.get("mock", "openai", "success"), 2);
    assert_eq!(c.get("mock", "openai", "error"), 1);
    assert_eq!(c.total(), 3);
}

#[test]
fn request_counter_snapshot() {
    let c = RequestCounter::new();
    c.increment("a", "d1", "ok");
    c.increment("b", "d2", "ok");
    let snap = c.snapshot();
    assert_eq!(snap.len(), 2);
}

#[test]
fn request_counter_reset() {
    let c = RequestCounter::new();
    c.increment("a", "d", "ok");
    c.reset();
    assert_eq!(c.total(), 0);
    assert!(c.snapshot().is_empty());
}

// ===========================================================================
// 11. ErrorCounter (metrics module)
// ===========================================================================

#[test]
fn error_counter_empty() {
    let c = ErrorCounter::new();
    assert_eq!(c.total(), 0);
    assert_eq!(c.get("E001"), 0);
}

#[test]
fn error_counter_increment_and_get() {
    let c = ErrorCounter::new();
    c.increment("timeout");
    c.increment("timeout");
    c.increment("rate_limit");
    assert_eq!(c.get("timeout"), 2);
    assert_eq!(c.get("rate_limit"), 1);
    assert_eq!(c.total(), 3);
}

#[test]
fn error_counter_snapshot() {
    let c = ErrorCounter::new();
    c.increment("e1");
    c.increment("e2");
    let snap = c.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap["e1"], 1);
}

#[test]
fn error_counter_reset() {
    let c = ErrorCounter::new();
    c.increment("e1");
    c.reset();
    assert_eq!(c.total(), 0);
}

// ===========================================================================
// 12. ActiveRequestGauge (metrics module)
// ===========================================================================

#[test]
fn gauge_starts_zero() {
    let g = ActiveRequestGauge::new();
    assert_eq!(g.get(), 0);
}

#[test]
fn gauge_increment() {
    let g = ActiveRequestGauge::new();
    g.increment();
    g.increment();
    assert_eq!(g.get(), 2);
}

#[test]
fn gauge_decrement() {
    let g = ActiveRequestGauge::new();
    g.increment();
    g.increment();
    g.decrement();
    assert_eq!(g.get(), 1);
}

#[test]
fn gauge_decrement_below_zero() {
    let g = ActiveRequestGauge::new();
    g.decrement();
    assert_eq!(g.get(), -1);
}

// ===========================================================================
// 13. TokenAccumulator (metrics module)
// ===========================================================================

#[test]
fn token_acc_empty() {
    let t = TokenAccumulator::new();
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
    assert_eq!(t.total(), 0);
}

#[test]
fn token_acc_add() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.add(50, 75);
    assert_eq!(t.total_input(), 150);
    assert_eq!(t.total_output(), 275);
    assert_eq!(t.total(), 425);
}

#[test]
fn token_acc_reset() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.reset();
    assert_eq!(t.total(), 0);
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
}

// ===========================================================================
// 14. Tracing spans (spans module)
// ===========================================================================

#[test]
fn request_span_no_panic() {
    let span = request_span("wo-123", "do stuff", "mapped");
    let _guard = span.enter();
}

#[test]
fn event_span_no_panic() {
    let span = event_span("tool_call", 7);
    let _guard = span.enter();
}

#[test]
fn backend_span_no_panic() {
    let span = backend_span("mock");
    let _guard = span.enter();
}

#[test]
fn nested_spans_no_panic() {
    let outer = request_span("wo-1", "task", "mapped");
    let _g1 = outer.enter();
    let inner = event_span("tool_call", 1);
    let _g2 = inner.enter();
}

#[test]
fn span_in_scope() {
    let span = backend_span("test_backend");
    span.in_scope(|| {
        let inner = event_span("message", 0);
        inner.in_scope(|| {
            // nested scope
        });
    });
}

// ===========================================================================
// 15. Pipeline — TelemetryEventType
// ===========================================================================

#[test]
fn event_type_display_run_started() {
    assert_eq!(TelemetryEventType::RunStarted.to_string(), "run_started");
}

#[test]
fn event_type_display_run_completed() {
    assert_eq!(
        TelemetryEventType::RunCompleted.to_string(),
        "run_completed"
    );
}

#[test]
fn event_type_display_run_failed() {
    assert_eq!(TelemetryEventType::RunFailed.to_string(), "run_failed");
}

#[test]
fn event_type_display_backend_selected() {
    assert_eq!(
        TelemetryEventType::BackendSelected.to_string(),
        "backend_selected"
    );
}

#[test]
fn event_type_display_retry_attempted() {
    assert_eq!(
        TelemetryEventType::RetryAttempted.to_string(),
        "retry_attempted"
    );
}

#[test]
fn event_type_display_fallback_triggered() {
    assert_eq!(
        TelemetryEventType::FallbackTriggered.to_string(),
        "fallback_triggered"
    );
}

#[test]
fn event_type_display_capability_negotiated() {
    assert_eq!(
        TelemetryEventType::CapabilityNegotiated.to_string(),
        "capability_negotiated"
    );
}

#[test]
fn event_type_display_mapping_performed() {
    assert_eq!(
        TelemetryEventType::MappingPerformed.to_string(),
        "mapping_performed"
    );
}

#[test]
fn event_type_serde_roundtrip() {
    let t = TelemetryEventType::RunStarted;
    let json = serde_json::to_string(&t).unwrap();
    let t2: TelemetryEventType = serde_json::from_str(&json).unwrap();
    assert_eq!(t, t2);
}

// ===========================================================================
// 16. Pipeline — TelemetryFilter
// ===========================================================================

#[test]
fn filter_default_matches_all() {
    let f = TelemetryFilter::default();
    let e = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_allowed_types_accept() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let e = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_allowed_types_reject() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let e = make_event(TelemetryEventType::RunFailed, None, None, None);
    assert!(!f.matches(&e));
}

#[test]
fn filter_min_duration_accept() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let e = make_event(TelemetryEventType::RunCompleted, None, None, Some(150));
    assert!(f.matches(&e));
}

#[test]
fn filter_min_duration_reject() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let e = make_event(TelemetryEventType::RunCompleted, None, None, Some(50));
    assert!(!f.matches(&e));
}

#[test]
fn filter_min_duration_none_duration_passes() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let e = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_combined_both_pass() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(10),
    };
    let e = make_event(TelemetryEventType::RunCompleted, None, None, Some(20));
    assert!(f.matches(&e));
}

#[test]
fn filter_combined_type_fails() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(10),
    };
    let e = make_event(TelemetryEventType::RunFailed, None, None, Some(20));
    assert!(!f.matches(&e));
}

// ===========================================================================
// 17. Pipeline — TelemetryCollector
// ===========================================================================

#[test]
fn collector_new_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
}

#[test]
fn collector_record_event() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        Some("mock"),
        None,
    ));
    assert_eq!(c.events().len(), 1);
}

#[test]
fn collector_with_filter_accepts() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(f);
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    assert_eq!(c.events().len(), 1);
}

#[test]
fn collector_with_filter_rejects() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(f);
    c.record(make_event(TelemetryEventType::RunFailed, None, None, None));
    assert!(c.events().is_empty());
}

#[test]
fn collector_events_of_type() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
}

#[test]
fn collector_run_events() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r2"),
        None,
        Some(100),
    ));
    c.record(make_event(
        TelemetryEventType::RunFailed,
        Some("r1"),
        None,
        None,
    ));
    let r1_events = c.run_events("r1");
    assert_eq!(r1_events.len(), 2);
}

#[test]
fn collector_summary_empty() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.error_rate, 0.0);
    assert!(s.average_run_duration_ms.is_none());
}

#[test]
fn collector_summary_with_runs() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(100),
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r2"),
        None,
        Some(200),
    ));
    c.record(make_event(
        TelemetryEventType::RunFailed,
        Some("r3"),
        None,
        None,
    ));
    let s = c.summary();
    assert_eq!(s.total_events, 3);
    assert_eq!(s.average_run_duration_ms, Some(150));
    // error_rate = 1 failed / (2 completed + 1 failed) = 1/3
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn collector_summary_events_by_type() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(
        TelemetryEventType::BackendSelected,
        None,
        None,
        None,
    ));
    let s = c.summary();
    assert_eq!(s.events_by_type["run_started"], 2);
    assert_eq!(s.events_by_type["backend_selected"], 1);
}

#[test]
fn collector_clear() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.clear();
    assert!(c.events().is_empty());
}

// ===========================================================================
// 18. Hooks — request lifecycle
// ===========================================================================

#[test]
fn hook_request_start_returns_instant() {
    let before = Instant::now();
    let start = on_request_start("wo-1", "mock");
    assert!(start >= before);
}

#[test]
fn hook_request_complete_success() {
    let start = Instant::now();
    let elapsed = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
    assert!(elapsed < 5000);
}

#[test]
fn hook_request_complete_error() {
    let start = Instant::now();
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    let elapsed = on_request_complete("wo-1", "mock", &outcome, start);
    assert!(elapsed < 5000);
}

#[test]
fn hook_on_error_transient() {
    on_error(
        "wo-1",
        "E001",
        "network blip",
        ErrorClassification::Transient,
    );
}

#[test]
fn hook_on_error_permanent() {
    on_error("wo-2", "E002", "bad auth", ErrorClassification::Permanent);
}

#[test]
fn hook_on_error_unknown() {
    on_error("wo-3", "E999", "mystery", ErrorClassification::Unknown);
}

#[test]
fn error_classification_display() {
    assert_eq!(ErrorClassification::Transient.to_string(), "transient");
    assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
    assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
}

#[test]
fn request_outcome_equality() {
    assert_eq!(RequestOutcome::Success, RequestOutcome::Success);
    let e1 = RequestOutcome::Error {
        code: "x".into(),
        message: "y".into(),
    };
    let e2 = RequestOutcome::Error {
        code: "x".into(),
        message: "y".into(),
    };
    assert_eq!(e1, e2);
    assert_ne!(RequestOutcome::Success, e1);
}

#[test]
fn error_classification_copy_clone() {
    let c = ErrorClassification::Transient;
    let c2 = c;
    assert_eq!(c, c2);
}

// ===========================================================================
// 19. Concurrent tracing from multiple threads
// ===========================================================================

#[test]
fn concurrent_metrics_collector_recording() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample("thread", i * 10, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
}

#[test]
fn concurrent_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(sample("pre", 10, 0));
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample("t", 20, 0));
            let _ = cc.summary();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 11);
}

#[test]
fn concurrent_request_counter() {
    let c = RequestCounter::new();
    let c1 = c.clone();
    let c2 = c.clone();
    let h1 = thread::spawn(move || {
        for _ in 0..100 {
            c1.increment("mock", "openai", "success");
        }
    });
    let h2 = thread::spawn(move || {
        for _ in 0..100 {
            c2.increment("mock", "openai", "success");
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    assert_eq!(c.get("mock", "openai", "success"), 200);
}

#[test]
fn concurrent_error_counter() {
    let c = ErrorCounter::new();
    let c1 = c.clone();
    let c2 = c.clone();
    let h1 = thread::spawn(move || {
        for _ in 0..50 {
            c1.increment("err_a");
        }
    });
    let h2 = thread::spawn(move || {
        for _ in 0..50 {
            c2.increment("err_b");
        }
    });
    h1.join().unwrap();
    h2.join().unwrap();
    assert_eq!(c.total(), 100);
}

#[test]
fn concurrent_gauge() {
    let g = Arc::new(ActiveRequestGauge::new());
    let mut handles = vec![];
    for _ in 0..10 {
        let gg = g.clone();
        handles.push(thread::spawn(move || {
            gg.increment();
            gg.decrement();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(g.get(), 0);
}

#[test]
fn concurrent_token_accumulator() {
    let t = Arc::new(TokenAccumulator::new());
    let mut handles = vec![];
    for _ in 0..10 {
        let tt = t.clone();
        handles.push(thread::spawn(move || {
            tt.add(10, 20);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(t.total_input(), 100);
    assert_eq!(t.total_output(), 200);
}

// ===========================================================================
// 20. Tracing subscriber configuration
// ===========================================================================

#[test]
fn tracing_info_span_creation() {
    let span = tracing::info_span!("test_span", key = "value");
    let _guard = span.enter();
}

#[test]
fn tracing_debug_span_creation() {
    let span = tracing::debug_span!("debug_span", num = 42);
    let _guard = span.enter();
}

#[test]
fn tracing_event_recording() {
    tracing::info!(target: "abp.test", msg = "hello", count = 5);
}

#[test]
fn tracing_warn_event() {
    tracing::warn!(target: "abp.test", error_code = "E001", "warning event");
}

#[test]
fn tracing_error_event() {
    tracing::error!(target: "abp.test", "error event");
}

#[test]
fn tracing_nested_spans() {
    let outer = tracing::info_span!("outer");
    let _g1 = outer.enter();
    let mid = tracing::info_span!("mid");
    let _g2 = mid.enter();
    let inner = tracing::info_span!("inner");
    let _g3 = inner.enter();
    tracing::info!("deeply nested event");
}

#[test]
fn tracing_span_with_structured_fields() {
    let span = tracing::info_span!(
        "structured",
        work_order_id = "wo-42",
        backend = "mock",
        tokens = 1000u64,
    );
    let _guard = span.enter();
    tracing::info!(duration_ms = 150u64, "completed");
}

// ===========================================================================
// 21. Additional edge cases
// ===========================================================================

#[test]
fn histogram_large_dataset() {
    let mut h = LatencyHistogram::new();
    for i in 0..10000 {
        h.record(i as f64);
    }
    assert_eq!(h.count(), 10000);
    assert_eq!(h.min(), Some(0.0));
    assert_eq!(h.max(), Some(9999.0));
    let mean = h.mean();
    assert!((mean - 4999.5).abs() < 0.1);
}

#[test]
fn run_summary_many_events() {
    let mut s = RunSummary::new();
    for _ in 0..1000 {
        s.record_event("tool_call");
    }
    for _ in 0..100 {
        s.record_event("error");
    }
    assert_eq!(s.total_events, 1100);
    assert_eq!(s.tool_call_count, 1000);
    assert_eq!(s.error_count, 100);
}

#[test]
fn cost_estimator_zero_tokens() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let cost = e.estimate("m", 0, 0).unwrap();
    assert_eq!(cost, 0.0);
}

#[test]
fn telemetry_event_with_metadata() {
    let mut e = make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        Some("mock"),
        None,
    );
    e.metadata
        .insert("key".to_string(), serde_json::json!("value"));
    e.metadata
        .insert("count".to_string(), serde_json::json!(42));
    assert_eq!(e.metadata.len(), 2);
}

#[test]
fn telemetry_collector_multiple_run_ids() {
    let mut c = TelemetryCollector::new();
    for i in 0..5 {
        let run_id = format!("run-{}", i);
        c.record(make_event(
            TelemetryEventType::RunStarted,
            Some(&run_id),
            None,
            None,
        ));
        c.record(make_event(
            TelemetryEventType::RunCompleted,
            Some(&run_id),
            None,
            Some(i as u64 * 100),
        ));
    }
    assert_eq!(c.events().len(), 10);
    assert_eq!(c.run_events("run-0").len(), 2);
    assert_eq!(c.run_events("run-4").len(), 2);
    assert_eq!(c.run_events("nonexistent").len(), 0);
}

#[test]
fn request_key_serde_roundtrip() {
    let key = RequestKey {
        backend: "mock".into(),
        dialect: "openai".into(),
        outcome: "success".into(),
    };
    let json = serde_json::to_string(&key).unwrap();
    let key2: RequestKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, key2);
}

#[test]
fn export_format_json_serde() {
    let json = serde_json::to_string(&ExportFormat::Json).unwrap();
    assert_eq!(json, "\"json\"");
}

#[test]
fn export_format_csv_serde() {
    let json = serde_json::to_string(&ExportFormat::Csv).unwrap();
    assert_eq!(json, "\"csv\"");
}

#[test]
fn export_format_structured_serde() {
    let json = serde_json::to_string(&ExportFormat::Structured).unwrap();
    assert_eq!(json, "\"structured\"");
}
