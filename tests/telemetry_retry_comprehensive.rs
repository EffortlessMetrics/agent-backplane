#![allow(clippy::all)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use abp_retry::{
    retry_with_policy, CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy,
};
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

// ============================================================================
// Helpers
// ============================================================================

fn sample_run(backend: &str, duration: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test_dialect".to_string(),
        duration_ms: duration,
        events_count: 10,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 3,
        errors_count: errors,
        emulations_applied: 1,
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
        run_id: run_id.map(|s| s.to_string()),
        backend: backend.map(|s| s.to_string()),
        metadata: BTreeMap::new(),
        duration_ms,
    }
}

// ============================================================================
// 1. MetricsCollector – basic operations
// ============================================================================

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
}

#[test]
fn collector_record_increments_len() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 10, 0));
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
}

#[test]
fn collector_record_multiple() {
    let c = MetricsCollector::new();
    for i in 0..5 {
        c.record(sample_run("b", i * 10, 0));
    }
    assert_eq!(c.len(), 5);
}

#[test]
fn collector_runs_returns_all_in_order() {
    let c = MetricsCollector::new();
    c.record(sample_run("first", 10, 0));
    c.record(sample_run("second", 20, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].backend_name, "first");
    assert_eq!(runs[1].backend_name, "second");
}

#[test]
fn collector_clear_empties() {
    let c = MetricsCollector::new();
    c.record(sample_run("x", 50, 0));
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 10, 0));
    c.clear();
    c.record(sample_run("b", 20, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

// ============================================================================
// 2. MetricsCollector – summary computation
// ============================================================================

#[test]
fn empty_collector_summary_defaults() {
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
fn single_run_summary() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 42, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, 42.0);
    assert_eq!(s.p50_duration_ms, 42.0);
    assert_eq!(s.p99_duration_ms, 42.0);
    assert_eq!(s.total_tokens_in, 100);
    assert_eq!(s.total_tokens_out, 200);
    assert_eq!(s.error_rate, 0.0);
    assert_eq!(s.backend_counts["mock"], 1);
}

#[test]
fn summary_mean_duration_multiple() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 100, 0));
    c.record(sample_run("a", 200, 0));
    c.record(sample_run("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_odd_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(sample_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_even_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(sample_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p99_large_dataset() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(sample_run("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn summary_total_tokens_aggregated() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 10, 0));
    c.record(sample_run("b", 20, 0));
    c.record(sample_run("c", 30, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 300);
    assert_eq!(s.total_tokens_out, 600);
}

#[test]
fn summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 10, 1));
    c.record(sample_run("a", 20, 3));
    let s = c.summary();
    // 4 errors / 2 runs = 2.0
    assert!((s.error_rate - 2.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 10, 0));
    c.record(sample_run("a", 20, 0));
    let s = c.summary();
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn summary_backend_counts_multiple() {
    let c = MetricsCollector::new();
    c.record(sample_run("alpha", 10, 0));
    c.record(sample_run("beta", 20, 0));
    c.record(sample_run("alpha", 30, 0));
    c.record(sample_run("gamma", 40, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 1);
    assert_eq!(s.backend_counts["gamma"], 1);
}

// ============================================================================
// 3. MetricsCollector – thread safety
// ============================================================================

#[test]
fn collector_concurrent_recording() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample_run("thread", i * 10, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn collector_concurrent_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(sample_run("pre", 10, 0));
    let mut handles = vec![];
    for _ in 0..5 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(sample_run("t", 20, 0));
            let _ = cc.summary();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 6);
}

// ============================================================================
// 4. RunMetrics serde
// ============================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = sample_run("serde_test", 999, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_default_zeroes() {
    let m = RunMetrics::default();
    assert_eq!(m.backend_name, "");
    assert_eq!(m.dialect, "");
    assert_eq!(m.duration_ms, 0);
    assert_eq!(m.tokens_in, 0);
    assert_eq!(m.tokens_out, 0);
    assert_eq!(m.errors_count, 0);
    assert_eq!(m.tool_calls_count, 0);
    assert_eq!(m.emulations_applied, 0);
    assert_eq!(m.events_count, 0);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(sample_run("a", 50, 1));
    c.record(sample_run("b", 100, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_default_serde() {
    let s = MetricsSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

// ============================================================================
// 5. TelemetrySpan
// ============================================================================

#[test]
fn telemetry_span_new() {
    let span = TelemetrySpan::new("test_op");
    assert_eq!(span.name, "test_op");
    assert!(span.attributes.is_empty());
}

#[test]
fn telemetry_span_with_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key1", "val1")
        .with_attribute("key2", "val2")
        .with_attribute("key3", "val3");
    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["key1"], "val1");
    assert_eq!(span.attributes["key2"], "val2");
    assert_eq!(span.attributes["key3"], "val3");
}

#[test]
fn telemetry_span_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "val1")
        .with_attribute("key", "val2");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "val2");
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("run").with_attribute("backend", "mock");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
}

#[test]
fn telemetry_span_emit_does_not_panic() {
    let span = TelemetrySpan::new("emit_test").with_attribute("a", "b");
    span.emit();
}

#[test]
fn telemetry_span_deterministic_attribute_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("zebra", "1")
        .with_attribute("alpha", "2");
    let json = serde_json::to_string(&span).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let zebra_pos = json.find("zebra").unwrap();
    assert!(alpha_pos < zebra_pos, "BTreeMap should produce sorted keys");
}

// ============================================================================
// 6. JsonExporter / TelemetryExporter trait
// ============================================================================

#[test]
fn json_exporter_valid_json() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 100, 0));
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
fn json_exporter_deterministic_backend_order() {
    let c = MetricsCollector::new();
    c.record(sample_run("zebra", 10, 0));
    c.record(sample_run("alpha", 20, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < zebra_pos);
}

// ============================================================================
// 7. MetricsExporter – multi-format
// ============================================================================

#[test]
fn metrics_exporter_json_format() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 100, 0));
    let s = c.summary();
    let json = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn metrics_exporter_csv_format() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 100, 0));
    let s = c.summary();
    let csv = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(csv.contains("count,mean_duration_ms"));
    assert!(csv.contains("1,"));
}

#[test]
fn metrics_exporter_structured_format() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 100, 0));
    let s = c.summary();
    let structured = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(structured.contains("count=1"));
    assert!(structured.contains("mean_duration_ms="));
    assert!(structured.contains("p50_duration_ms="));
    assert!(structured.contains("p99_duration_ms="));
}

#[test]
fn metrics_exporter_csv_runs() {
    let runs = vec![sample_run("mock", 100, 0), sample_run("sidecar", 200, 1)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    assert!(csv.contains("backend_name,dialect,duration_ms"));
    assert!(csv.contains("mock,test_dialect,100"));
    assert!(csv.contains("sidecar,test_dialect,200"));
}

#[test]
fn metrics_exporter_structured_includes_backends() {
    let c = MetricsCollector::new();
    c.record(sample_run("mock", 100, 0));
    let s = c.summary();
    let structured = MetricsExporter::export_structured(&s).unwrap();
    assert!(structured.contains("backend.mock=1"));
}

#[test]
fn export_format_serde_roundtrip() {
    for fmt in [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Structured,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let fmt2: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, fmt2);
    }
}

#[test]
fn export_format_json_snake_case() {
    let json = serde_json::to_string(&ExportFormat::Json).unwrap();
    assert_eq!(json, "\"json\"");
}

#[test]
fn export_format_csv_snake_case() {
    let json = serde_json::to_string(&ExportFormat::Csv).unwrap();
    assert_eq!(json, "\"csv\"");
}

#[test]
fn export_format_structured_snake_case() {
    let json = serde_json::to_string(&ExportFormat::Structured).unwrap();
    assert_eq!(json, "\"structured\"");
}

// ============================================================================
// 8. RunSummary
// ============================================================================

#[test]
fn run_summary_new_is_empty() {
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
    assert_eq!(s.total_events, 1);
}

#[test]
fn run_summary_record_generic_event() {
    let mut s = RunSummary::new();
    s.record_event("info");
    assert_eq!(s.total_events, 1);
    assert_eq!(s.error_count, 0);
    assert_eq!(s.warning_count, 0);
    assert_eq!(s.tool_call_count, 0);
    assert_eq!(s.event_counts["info"], 1);
}

#[test]
fn run_summary_set_duration() {
    let mut s = RunSummary::new();
    s.set_duration(12345);
    assert_eq!(s.total_duration_ms, 12345);
}

#[test]
fn run_summary_has_errors_false_when_none() {
    let s = RunSummary::new();
    assert!(!s.has_errors());
}

#[test]
fn run_summary_error_rate_zero_events() {
    let s = RunSummary::new();
    assert_eq!(s.error_rate(), 0.0);
}

#[test]
fn run_summary_error_rate_some_errors() {
    let mut s = RunSummary::new();
    s.record_event("error");
    s.record_event("info");
    s.record_event("info");
    s.record_event("info");
    // 1 error / 4 total = 0.25
    assert!((s.error_rate() - 0.25).abs() < f64::EPSILON);
}

#[test]
fn run_summary_merge() {
    let mut s1 = RunSummary::new();
    s1.record_event("error");
    s1.set_duration(100);

    let mut s2 = RunSummary::new();
    s2.record_event("tool_call");
    s2.record_event("error");
    s2.set_duration(200);

    s1.merge(&s2);
    assert_eq!(s1.total_events, 3);
    assert_eq!(s1.error_count, 2);
    assert_eq!(s1.tool_call_count, 1);
    assert_eq!(s1.total_duration_ms, 300);
}

#[test]
fn run_summary_from_events() {
    let s = RunSummary::from_events(&["error", "tool_call", "info", "error"], 5000);
    assert_eq!(s.total_events, 4);
    assert_eq!(s.error_count, 2);
    assert_eq!(s.tool_call_count, 1);
    assert_eq!(s.total_duration_ms, 5000);
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["error", "tool_call"], 1000);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

// ============================================================================
// 9. LatencyHistogram
// ============================================================================

#[test]
fn histogram_new_is_empty() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.count(), 0);
}

#[test]
fn histogram_record_values() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    assert_eq!(h.count(), 2);
    assert!(!h.is_empty());
}

#[test]
fn histogram_min_max() {
    let mut h = LatencyHistogram::new();
    h.record(5.0);
    h.record(15.0);
    h.record(10.0);
    assert_eq!(h.min(), Some(5.0));
    assert_eq!(h.max(), Some(15.0));
}

#[test]
fn histogram_min_max_empty() {
    let h = LatencyHistogram::new();
    assert_eq!(h.min(), None);
    assert_eq!(h.max(), None);
}

#[test]
fn histogram_mean() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);
    assert!((h.mean() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_mean_empty() {
    let h = LatencyHistogram::new();
    assert_eq!(h.mean(), 0.0);
}

#[test]
fn histogram_p50() {
    let mut h = LatencyHistogram::new();
    for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
        h.record(v);
    }
    assert!((h.p50() - 30.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_p95_p99() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    assert!(h.p95() > 94.0);
    assert!(h.p99() > 98.0);
}

#[test]
fn histogram_percentile_empty() {
    let h = LatencyHistogram::new();
    assert_eq!(h.percentile(50.0), 0.0);
    assert_eq!(h.percentile(99.0), 0.0);
}

#[test]
fn histogram_percentile_single_value() {
    let mut h = LatencyHistogram::new();
    h.record(42.0);
    assert_eq!(h.percentile(0.0), 42.0);
    assert_eq!(h.percentile(50.0), 42.0);
    assert_eq!(h.percentile(100.0), 42.0);
}

#[test]
fn histogram_merge() {
    let mut h1 = LatencyHistogram::new();
    h1.record(10.0);
    h1.record(20.0);

    let mut h2 = LatencyHistogram::new();
    h2.record(30.0);
    h2.record(40.0);

    h1.merge(&h2);
    assert_eq!(h1.count(), 4);
    assert_eq!(h1.min(), Some(10.0));
    assert_eq!(h1.max(), Some(40.0));
}

#[test]
fn histogram_buckets() {
    let mut h = LatencyHistogram::new();
    for v in [5.0, 15.0, 25.0, 35.0, 100.0] {
        h.record(v);
    }
    let boundaries = [10.0, 20.0, 30.0, 50.0];
    let counts = h.buckets(&boundaries);
    // [0,10): 5.0 => 1
    // [10,20): 15.0 => 1
    // [20,30): 25.0 => 1
    // [30,50): 35.0 => 1
    // [50,∞): 100.0 => 1
    assert_eq!(counts, vec![1, 1, 1, 1, 1]);
}

#[test]
fn histogram_buckets_empty() {
    let h = LatencyHistogram::new();
    let counts = h.buckets(&[10.0, 20.0]);
    assert_eq!(counts, vec![0, 0, 0]);
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

// ============================================================================
// 10. CostEstimator
// ============================================================================

#[test]
fn cost_estimator_new_empty() {
    let e = CostEstimator::new();
    assert!(e.models().is_empty());
}

#[test]
fn cost_estimator_set_and_get_pricing() {
    let mut e = CostEstimator::new();
    let pricing = ModelPricing {
        input_cost_per_token: 0.001,
        output_cost_per_token: 0.002,
    };
    e.set_pricing("gpt-4", pricing.clone());
    let got = e.get_pricing("gpt-4").unwrap();
    assert_eq!(*got, pricing);
}

#[test]
fn cost_estimator_get_pricing_missing() {
    let e = CostEstimator::new();
    assert!(e.get_pricing("nonexistent").is_none());
}

#[test]
fn cost_estimator_estimate() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.03,
        },
    );
    // 100 * 0.01 + 50 * 0.03 = 1.0 + 1.5 = 2.5
    let cost = e.estimate("gpt-4", 100, 50).unwrap();
    assert!((cost - 2.5).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_estimate_unknown_model() {
    let e = CostEstimator::new();
    assert!(e.estimate("unknown", 100, 50).is_none());
}

#[test]
fn cost_estimator_estimate_total() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.03,
        },
    );
    e.set_pricing(
        "claude",
        ModelPricing {
            input_cost_per_token: 0.02,
            output_cost_per_token: 0.04,
        },
    );
    let total = e.estimate_total(&[("gpt-4", 100, 50), ("claude", 200, 100)]);
    // gpt-4: 100*0.01 + 50*0.03 = 2.5
    // claude: 200*0.02 + 100*0.04 = 8.0
    assert!((total - 10.5).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_estimate_total_skips_unknown() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.03,
        },
    );
    let total = e.estimate_total(&[("gpt-4", 100, 50), ("unknown", 200, 100)]);
    assert!((total - 2.5).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_models_list() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "b",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    e.set_pricing(
        "a",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    let models = e.models();
    // BTreeMap ensures sorted order
    assert_eq!(models, vec!["a", "b"]);
}

#[test]
fn cost_estimator_serde_roundtrip() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.03,
        },
    );
    let json = serde_json::to_string(&e).unwrap();
    let e2: CostEstimator = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.models(), vec!["gpt-4"]);
}

// ============================================================================
// 11. RequestCounter (metrics)
// ============================================================================

#[test]
fn request_counter_empty() {
    let c = RequestCounter::new();
    assert_eq!(c.total(), 0);
    assert_eq!(c.get("x", "y", "z"), 0);
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

#[test]
fn request_counter_concurrent() {
    let c = RequestCounter::new();
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.increment("mock", "openai", "success");
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.get("mock", "openai", "success"), 10);
}

#[test]
fn request_key_serde_roundtrip() {
    let key = RequestKey {
        backend: "mock".to_string(),
        dialect: "openai".to_string(),
        outcome: "success".to_string(),
    };
    let json = serde_json::to_string(&key).unwrap();
    let key2: RequestKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, key2);
}

// ============================================================================
// 12. ErrorCounter (metrics)
// ============================================================================

#[test]
fn error_counter_empty() {
    let c = ErrorCounter::new();
    assert_eq!(c.total(), 0);
    assert_eq!(c.get("any_code"), 0);
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
    c.increment("timeout");
    c.increment("internal");
    let snap = c.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap["timeout"], 1);
    assert_eq!(snap["internal"], 1);
}

#[test]
fn error_counter_reset() {
    let c = ErrorCounter::new();
    c.increment("timeout");
    c.reset();
    assert_eq!(c.total(), 0);
    assert!(c.snapshot().is_empty());
}

#[test]
fn error_counter_concurrent() {
    let c = ErrorCounter::new();
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.increment("timeout");
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.get("timeout"), 10);
}

// ============================================================================
// 13. ActiveRequestGauge (metrics)
// ============================================================================

#[test]
fn gauge_starts_at_zero() {
    let g = ActiveRequestGauge::new();
    assert_eq!(g.get(), 0);
}

#[test]
fn gauge_default_starts_at_zero() {
    let g = ActiveRequestGauge::default();
    assert_eq!(g.get(), 0);
}

#[test]
fn gauge_increment_and_decrement() {
    let g = ActiveRequestGauge::new();
    g.increment();
    g.increment();
    g.increment();
    assert_eq!(g.get(), 3);
    g.decrement();
    assert_eq!(g.get(), 2);
    g.decrement();
    g.decrement();
    assert_eq!(g.get(), 0);
}

#[test]
fn gauge_can_go_negative() {
    let g = ActiveRequestGauge::new();
    g.decrement();
    assert_eq!(g.get(), -1);
}

// ============================================================================
// 14. TokenAccumulator (metrics)
// ============================================================================

#[test]
fn token_accumulator_empty() {
    let t = TokenAccumulator::new();
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
    assert_eq!(t.total(), 0);
}

#[test]
fn token_accumulator_add() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.add(50, 75);
    assert_eq!(t.total_input(), 150);
    assert_eq!(t.total_output(), 275);
    assert_eq!(t.total(), 425);
}

#[test]
fn token_accumulator_reset() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.reset();
    assert_eq!(t.total(), 0);
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
}

#[test]
fn token_accumulator_add_only_input() {
    let t = TokenAccumulator::new();
    t.add(500, 0);
    assert_eq!(t.total_input(), 500);
    assert_eq!(t.total_output(), 0);
    assert_eq!(t.total(), 500);
}

#[test]
fn token_accumulator_add_only_output() {
    let t = TokenAccumulator::new();
    t.add(0, 300);
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 300);
    assert_eq!(t.total(), 300);
}

// ============================================================================
// 15. Tracing spans
// ============================================================================

#[test]
fn request_span_does_not_panic() {
    let span = request_span("wo-123", "do stuff", "mapped");
    let _guard = span.enter();
}

#[test]
fn event_span_does_not_panic() {
    let span = event_span("tool_call", 7);
    let _guard = span.enter();
}

#[test]
fn backend_span_does_not_panic() {
    let span = backend_span("mock");
    let _guard = span.enter();
}

#[test]
fn multiple_spans_nested() {
    let outer = request_span("wo-1", "task", "passthrough");
    let _outer_guard = outer.enter();
    let inner = event_span("tool_call", 1);
    let _inner_guard = inner.enter();
}

#[test]
fn backend_span_various_names() {
    for name in [
        "mock",
        "sidecar:node",
        "sidecar:claude",
        "openai",
        "anthropic",
    ] {
        let span = backend_span(name);
        let _guard = span.enter();
    }
}

// ============================================================================
// 16. Hooks – RequestOutcome, ErrorClassification
// ============================================================================

#[test]
fn request_outcome_success_equality() {
    assert_eq!(RequestOutcome::Success, RequestOutcome::Success);
}

#[test]
fn request_outcome_error_equality() {
    let e1 = RequestOutcome::Error {
        code: "timeout".into(),
        message: "timed out".into(),
    };
    let e2 = RequestOutcome::Error {
        code: "timeout".into(),
        message: "timed out".into(),
    };
    assert_eq!(e1, e2);
}

#[test]
fn request_outcome_success_ne_error() {
    let e = RequestOutcome::Error {
        code: "x".into(),
        message: "y".into(),
    };
    assert_ne!(RequestOutcome::Success, e);
}

#[test]
fn error_classification_display() {
    assert_eq!(ErrorClassification::Transient.to_string(), "transient");
    assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
    assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
}

#[test]
fn error_classification_equality() {
    assert_eq!(
        ErrorClassification::Transient,
        ErrorClassification::Transient
    );
    assert_ne!(
        ErrorClassification::Transient,
        ErrorClassification::Permanent
    );
    assert_ne!(ErrorClassification::Permanent, ErrorClassification::Unknown);
}

#[test]
fn on_request_start_returns_instant() {
    let before = Instant::now();
    let start = on_request_start("wo-1", "mock");
    assert!(start >= before);
}

#[test]
fn on_request_complete_success_returns_elapsed() {
    let start = Instant::now();
    let elapsed = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
    assert!(elapsed < 1000);
}

#[test]
fn on_request_complete_error_returns_elapsed() {
    let start = Instant::now();
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    let elapsed = on_request_complete("wo-1", "mock", &outcome, start);
    assert!(elapsed < 1000);
}

#[test]
fn on_error_does_not_panic() {
    on_error(
        "wo-1",
        "E001",
        "something broke",
        ErrorClassification::Transient,
    );
    on_error("wo-2", "E002", "bad input", ErrorClassification::Permanent);
    on_error("wo-3", "E999", "mystery", ErrorClassification::Unknown);
}

// ============================================================================
// 17. Pipeline – TelemetryEventType
// ============================================================================

#[test]
fn telemetry_event_type_display() {
    assert_eq!(TelemetryEventType::RunStarted.to_string(), "run_started");
    assert_eq!(
        TelemetryEventType::RunCompleted.to_string(),
        "run_completed"
    );
    assert_eq!(TelemetryEventType::RunFailed.to_string(), "run_failed");
    assert_eq!(
        TelemetryEventType::BackendSelected.to_string(),
        "backend_selected"
    );
    assert_eq!(
        TelemetryEventType::RetryAttempted.to_string(),
        "retry_attempted"
    );
    assert_eq!(
        TelemetryEventType::FallbackTriggered.to_string(),
        "fallback_triggered"
    );
    assert_eq!(
        TelemetryEventType::CapabilityNegotiated.to_string(),
        "capability_negotiated"
    );
    assert_eq!(
        TelemetryEventType::MappingPerformed.to_string(),
        "mapping_performed"
    );
}

#[test]
fn telemetry_event_type_serde_roundtrip() {
    for et in [
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunCompleted,
        TelemetryEventType::RunFailed,
        TelemetryEventType::BackendSelected,
        TelemetryEventType::RetryAttempted,
        TelemetryEventType::FallbackTriggered,
        TelemetryEventType::CapabilityNegotiated,
        TelemetryEventType::MappingPerformed,
    ] {
        let json = serde_json::to_string(&et).unwrap();
        let et2: TelemetryEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(et, et2);
    }
}

// ============================================================================
// 18. Pipeline – TelemetryFilter
// ============================================================================

#[test]
fn filter_default_matches_all() {
    let f = TelemetryFilter::default();
    let ev = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&ev));
}

#[test]
fn filter_allowed_types_passes() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let ev = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&ev));
}

#[test]
fn filter_allowed_types_rejects() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let ev = make_event(TelemetryEventType::RunFailed, None, None, None);
    assert!(!f.matches(&ev));
}

#[test]
fn filter_min_duration_passes() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let ev = make_event(TelemetryEventType::RunCompleted, None, None, Some(200));
    assert!(f.matches(&ev));
}

#[test]
fn filter_min_duration_rejects() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let ev = make_event(TelemetryEventType::RunCompleted, None, None, Some(50));
    assert!(!f.matches(&ev));
}

#[test]
fn filter_min_duration_none_duration_passes() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    let ev = make_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&ev));
}

#[test]
fn filter_combined_allowed_and_duration() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(50),
    };
    let ev = make_event(TelemetryEventType::RunCompleted, None, None, Some(100));
    assert!(f.matches(&ev));

    let ev2 = make_event(TelemetryEventType::RunCompleted, None, None, Some(10));
    assert!(!f.matches(&ev2));

    let ev3 = make_event(TelemetryEventType::RunFailed, None, None, Some(100));
    assert!(!f.matches(&ev3));
}

// ============================================================================
// 19. Pipeline – TelemetryCollector
// ============================================================================

#[test]
fn collector_pipeline_new_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
}

#[test]
fn collector_pipeline_record_events() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(100),
    ));
    assert_eq!(c.events().len(), 2);
}

#[test]
fn collector_pipeline_with_filter() {
    let filter = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(filter);
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].event_type, TelemetryEventType::RunCompleted);
}

#[test]
fn collector_pipeline_events_of_type() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(TelemetryEventType::RunFailed, None, None, None));
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
}

#[test]
fn collector_pipeline_run_events() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r2"),
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(100),
    ));
    let r1_events = c.run_events("r1");
    assert_eq!(r1_events.len(), 2);
}

#[test]
fn collector_pipeline_summary_empty() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert!(s.average_run_duration_ms.is_none());
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn collector_pipeline_summary_counts() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    c.record(make_event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.events_by_type["run_started"], 1);
    assert_eq!(s.events_by_type["run_completed"], 2);
    assert_eq!(s.events_by_type["run_failed"], 1);
}

#[test]
fn collector_pipeline_summary_avg_duration() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(150));
}

#[test]
fn collector_pipeline_summary_error_rate() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(make_event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    // 1 failed / (1 completed + 1 failed) = 0.5
    assert!((s.error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn collector_pipeline_clear() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.clear();
    assert!(c.events().is_empty());
}

#[test]
fn telemetry_summary_serde_roundtrip() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: TelemetrySummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s2.total_events, s.total_events);
}

// ============================================================================
// 20. RetryPolicy – construction and defaults
// ============================================================================

#[test]
fn retry_policy_default() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
    assert_eq!(p.base_delay, Duration::from_millis(100));
    assert_eq!(p.max_delay, Duration::from_secs(5));
    assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    assert!(p.jitter);
}

#[test]
fn retry_policy_no_retry() {
    let p = RetryPolicy::no_retry();
    assert_eq!(p.max_retries, 0);
    assert_eq!(p.base_delay, Duration::ZERO);
    assert_eq!(p.max_delay, Duration::ZERO);
    assert!(!p.jitter);
}

#[test]
fn retry_policy_custom() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(200),
        Duration::from_secs(10),
        3.0,
        false,
    );
    assert_eq!(p.max_retries, 5);
    assert_eq!(p.base_delay, Duration::from_millis(200));
    assert_eq!(p.max_delay, Duration::from_secs(10));
    assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
    assert!(!p.jitter);
}

#[test]
fn retry_policy_clone_eq() {
    let p = RetryPolicy::default();
    let p2 = p.clone();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_debug() {
    let p = RetryPolicy::default();
    let dbg = format!("{p:?}");
    assert!(dbg.contains("RetryPolicy"));
}

#[test]
fn retry_policy_serde_roundtrip() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_no_retry_serde_roundtrip() {
    let p = RetryPolicy::no_retry();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_custom_serde_roundtrip() {
    let p = RetryPolicy::new(
        7,
        Duration::from_millis(250),
        Duration::from_secs(30),
        1.5,
        false,
    );
    let json = serde_json::to_string_pretty(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RetryPolicy>();
}

// ============================================================================
// 21. RetryPolicy – backoff delay
// ============================================================================

#[test]
fn delay_attempt_zero_no_jitter() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
}

#[test]
fn delay_attempt_one_no_jitter() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(200));
}

#[test]
fn delay_attempt_two_no_jitter() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(400));
}

#[test]
fn delay_capped_at_max() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(5), Duration::from_secs(5));
}

#[test]
fn delay_with_multiplier_three() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(60),
        3.0,
        false,
    );
    // attempt 2: 100ms * 3^2 = 900ms
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(900));
}

#[test]
fn delay_exponential_growth() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(50),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let d0 = p.delay_for_attempt(0);
    let d1 = p.delay_for_attempt(1);
    let d2 = p.delay_for_attempt(2);
    let d3 = p.delay_for_attempt(3);
    assert!(d1 > d0);
    assert!(d2 > d1);
    assert!(d3 > d2);
}

#[test]
fn delay_no_jitter_deterministic() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let d1 = p.delay_for_attempt(1);
    let d2 = p.delay_for_attempt(1);
    assert_eq!(d1, d2);
}

// ============================================================================
// 22. RetryPolicy – jitter
// ============================================================================

#[test]
fn jitter_produces_varying_delays() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        true,
    );
    let delays: Vec<Duration> = (0..20).map(|_| p.delay_for_attempt(1)).collect();
    let all_same = delays.windows(2).all(|w| w[0] == w[1]);
    assert!(!all_same, "jitter should produce varying delays");
}

#[test]
fn jitter_delay_bounded() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        true,
    );
    for _ in 0..100 {
        let d = p.delay_for_attempt(1);
        assert!(d <= Duration::from_millis(200));
    }
}

#[test]
fn jitter_delay_at_cap_bounded() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        true,
    );
    for _ in 0..100 {
        let d = p.delay_for_attempt(5);
        assert!(d <= Duration::from_secs(5));
    }
}

// ============================================================================
// 23. retry_with_policy – async tests
// ============================================================================

#[tokio::test]
async fn retry_succeeds_first_attempt() {
    let p = RetryPolicy::no_retry();
    let result = retry_with_policy(&p, || async { Ok::<_, String>(42) }).await;
    assert_eq!(result.unwrap(), 42);
}

#[tokio::test]
async fn retry_succeeds_after_failures() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err("not yet")
            } else {
                Ok("done")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "done");
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_exhausts_all_attempts() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let result: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_returns_last_error() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let result: Result<(), String> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Err(format!("err-{n}"))
        }
    })
    .await;
    assert_eq!(result.unwrap_err(), "err-2");
}

#[tokio::test]
async fn retry_no_retry_fails_immediately() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::no_retry();
    let c = counter.clone();
    let result: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_succeeds_on_last_attempt() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 3 {
                Err("not yet")
            } else {
                Ok("last")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "last");
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn retry_with_different_error_types_string() {
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<(), String> =
        retry_with_policy(&p, || async { Err("string error".to_string()) }).await;
    assert_eq!(result.unwrap_err(), "string error");
}

#[tokio::test]
async fn retry_with_different_error_types_io() {
    let p = RetryPolicy::no_retry();
    let result: Result<(), std::io::Error> = retry_with_policy(&p, || async {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out",
        ))
    })
    .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn retry_with_max_retries_one() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn retry_with_high_max_retries_succeeds_early() {
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        100,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err("first fail")
            } else {
                Ok("ok")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "ok");
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

// ============================================================================
// 24. CircuitBreaker – construction and state
// ============================================================================

#[test]
fn circuit_breaker_starts_closed() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[test]
fn circuit_breaker_initial_failures_zero() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    assert_eq!(cb.consecutive_failures(), 0);
}

#[test]
fn circuit_breaker_threshold_getter() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(10));
    assert_eq!(cb.failure_threshold(), 5);
}

#[test]
fn circuit_breaker_recovery_timeout_getter() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(42));
    assert_eq!(cb.recovery_timeout(), Duration::from_secs(42));
}

#[test]
fn circuit_breaker_debug() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let dbg = format!("{cb:?}");
    assert!(dbg.contains("CircuitBreaker"));
}

#[test]
fn circuit_breaker_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CircuitBreaker>();
}

// ============================================================================
// 25. CircuitBreaker – async behavior
// ============================================================================

#[tokio::test]
async fn circuit_breaker_passes_success() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("ok".to_string()) })
        .await;
    assert_eq!(res.unwrap(), "ok");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn circuit_breaker_records_failure() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("boom") }).await;
    assert_eq!(cb.consecutive_failures(), 1);
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn circuit_breaker_opens_after_threshold() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(30));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn circuit_breaker_rejects_when_open() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("should not run".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
}

#[tokio::test]
async fn circuit_breaker_half_open_after_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("probe".into()) }).await;
    assert_eq!(res.unwrap(), "probe");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn circuit_breaker_closes_after_successful_probe() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert!(res.is_ok());
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn circuit_breaker_reopens_after_failed_probe() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still broken") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn circuit_breaker_success_resets_failure_count() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.consecutive_failures(), 1);
    let _: Result<String, _> = cb
        .call(|| async { Ok::<_, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn circuit_breaker_inner_error_returned() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Err("details".to_string()) }).await;
    match res {
        Err(CircuitBreakerError::Inner(e)) => assert_eq!(e, "details"),
        other => panic!("expected Inner error, got {other:?}"),
    }
}

// ============================================================================
// 26. CircuitState serde
// ============================================================================

#[test]
fn circuit_state_serde_roundtrip() {
    for state in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let s2: CircuitState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, s2);
    }
}

#[test]
fn circuit_state_snake_case() {
    assert_eq!(
        serde_json::to_string(&CircuitState::Closed).unwrap(),
        "\"closed\""
    );
    assert_eq!(
        serde_json::to_string(&CircuitState::Open).unwrap(),
        "\"open\""
    );
    assert_eq!(
        serde_json::to_string(&CircuitState::HalfOpen).unwrap(),
        "\"half_open\""
    );
}

#[test]
fn circuit_state_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CircuitState>();
}

// ============================================================================
// 27. CircuitBreakerError
// ============================================================================

#[test]
fn circuit_breaker_error_open_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
    assert_eq!(e.to_string(), "circuit breaker is open");
}

#[test]
fn circuit_breaker_error_inner_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Inner("something broke".to_string());
    assert_eq!(e.to_string(), "something broke");
}

// ============================================================================
// 28. Integration: retry + circuit breaker + telemetry
// ============================================================================

#[tokio::test]
async fn retry_records_in_telemetry_collector() {
    let collector = MetricsCollector::new();
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let c = counter.clone();

    let start = Instant::now();
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 1 {
                Err("transient")
            } else {
                Ok("ok")
            }
        }
    })
    .await;
    let elapsed = start.elapsed().as_millis() as u64;

    assert!(result.is_ok());
    let errors = if counter.load(Ordering::SeqCst) > 1 {
        1
    } else {
        0
    };
    collector.record(RunMetrics {
        backend_name: "test_backend".to_string(),
        dialect: "test".to_string(),
        duration_ms: elapsed,
        events_count: counter.load(Ordering::SeqCst) as u64,
        tokens_in: 50,
        tokens_out: 100,
        tool_calls_count: 0,
        errors_count: errors,
        emulations_applied: 0,
    });

    assert_eq!(collector.len(), 1);
    let summary = collector.summary();
    assert_eq!(summary.count, 1);
}

#[tokio::test]
async fn circuit_breaker_with_retry_pattern() {
    // CircuitBreaker is not Clone, so we test the pattern sequentially.
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let counter = Arc::new(AtomicU32::new(0));

    // First call fails
    let c = counter.clone();
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("transient error".to_string())
            }
        })
        .await;
    assert!(res.is_err());
    assert_eq!(cb.state(), CircuitState::Closed);

    // Second call succeeds
    let c = counter.clone();
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("success".to_string())
            }
        })
        .await;
    assert_eq!(res.unwrap(), "success");
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

// ============================================================================
// 29. TelemetryFilter serde
// ============================================================================

#[test]
fn telemetry_filter_serde_roundtrip() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![
            TelemetryEventType::RunStarted,
            TelemetryEventType::RunCompleted,
        ]),
        min_duration_ms: Some(100),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: TelemetryFilter = serde_json::from_str(&json).unwrap();
    assert_eq!(f2.allowed_types, f.allowed_types);
    assert_eq!(f2.min_duration_ms, f.min_duration_ms);
}

#[test]
fn telemetry_filter_default_serde() {
    let f = TelemetryFilter::default();
    let json = serde_json::to_string(&f).unwrap();
    let f2: TelemetryFilter = serde_json::from_str(&json).unwrap();
    assert!(f2.allowed_types.is_none());
    assert!(f2.min_duration_ms.is_none());
}

// ============================================================================
// 30. TelemetryEvent serde
// ============================================================================

#[test]
fn telemetry_event_serde_roundtrip() {
    let mut metadata = BTreeMap::new();
    metadata.insert("key".to_string(), serde_json::json!("value"));
    let ev = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::RunCompleted,
        run_id: Some("run-1".to_string()),
        backend: Some("mock".to_string()),
        metadata,
        duration_ms: Some(500),
    };
    let json = serde_json::to_string(&ev).unwrap();
    let ev2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev2.event_type, TelemetryEventType::RunCompleted);
    assert_eq!(ev2.run_id, Some("run-1".to_string()));
    assert_eq!(ev2.duration_ms, Some(500));
}

// ============================================================================
// 31. Edge cases
// ============================================================================

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
    assert!((mean - 4999.5).abs() < 0.01);
}

#[test]
fn run_summary_merge_empty() {
    let mut s1 = RunSummary::from_events(&["error"], 100);
    let s2 = RunSummary::new();
    s1.merge(&s2);
    assert_eq!(s1.total_events, 1);
    assert_eq!(s1.error_count, 1);
    assert_eq!(s1.total_duration_ms, 100);
}

#[test]
fn cost_estimator_zero_tokens() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.03,
        },
    );
    let cost = e.estimate("gpt-4", 0, 0).unwrap();
    assert_eq!(cost, 0.0);
}

#[test]
fn collector_many_runs_summary() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(sample_run("backend", i, if i % 10 == 0 { 1 } else { 0 }));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    assert_eq!(s.backend_counts["backend"], 1000);
}

#[test]
fn telemetry_event_with_metadata() {
    let mut metadata = BTreeMap::new();
    metadata.insert("attempt".to_string(), serde_json::json!(3));
    metadata.insert("backend".to_string(), serde_json::json!("mock"));
    let ev = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::RetryAttempted,
        run_id: Some("run-1".to_string()),
        backend: Some("mock".to_string()),
        metadata,
        duration_ms: None,
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("retry_attempted"));
    assert!(json.contains("\"attempt\":3"));
}

#[test]
fn model_pricing_serde_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.001,
        output_cost_per_token: 0.002,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}
