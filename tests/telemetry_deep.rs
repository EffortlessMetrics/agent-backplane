#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Deep tests for `abp-telemetry`: spans, metrics, histogram, cost estimation,
//! exporters, and observability integration.
#![allow(clippy::float_cmp)]

use std::thread;

use abp_telemetry::{
    CostEstimator, ExportFormat, LatencyHistogram, MetricsCollector, MetricsExporter,
    MetricsSummary, ModelPricing, RunMetrics, RunSummary, TelemetrySpan,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample(
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

// =========================================================================
//  1. Span types and construction (10 tests)
// =========================================================================

#[test]
fn span_create_with_name() {
    let span = TelemetrySpan::new("agent.run");
    assert_eq!(span.name, "agent.run");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_create_with_parent_attribute() {
    let parent = TelemetrySpan::new("parent").with_attribute("span_id", "s-parent");
    let child = TelemetrySpan::new("child")
        .with_attribute("parent_span_id", parent.attributes["span_id"].clone())
        .with_attribute("span_id", "s-child");

    assert_eq!(child.attributes["parent_span_id"], "s-parent");
    assert_eq!(child.attributes["span_id"], "s-child");
}

#[test]
fn span_timing_start_end() {
    let start = std::time::Instant::now();
    let span = TelemetrySpan::new("timed").with_attribute("start_ms", "0");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let elapsed = start.elapsed().as_millis();
    let span = span.with_attribute("end_ms", elapsed.to_string());

    let end_val: u128 = span.attributes["end_ms"].parse().unwrap();
    assert!(
        end_val >= 5,
        "elapsed should be at least 5ms, got {end_val}"
    );
}

#[test]
fn span_attributes_multiple() {
    let span = TelemetrySpan::new("db.query")
        .with_attribute("db.system", "postgres")
        .with_attribute("db.statement", "SELECT 1")
        .with_attribute("db.name", "mydb");

    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["db.system"], "postgres");
}

#[test]
fn span_status_ok_and_error() {
    let ok_span = TelemetrySpan::new("op.ok").with_attribute("status", "ok");
    let err_span = TelemetrySpan::new("op.err")
        .with_attribute("status", "error")
        .with_attribute("error.message", "timeout");

    assert_eq!(ok_span.attributes["status"], "ok");
    assert_eq!(err_span.attributes["status"], "error");
    assert_eq!(err_span.attributes["error.message"], "timeout");
}

#[test]
fn span_nested_hierarchy() {
    let root = TelemetrySpan::new("root")
        .with_attribute("trace_id", "t-1")
        .with_attribute("span_id", "s-1");
    let child = TelemetrySpan::new("child")
        .with_attribute("trace_id", root.attributes["trace_id"].clone())
        .with_attribute("parent_span_id", root.attributes["span_id"].clone())
        .with_attribute("span_id", "s-2");
    let grandchild = TelemetrySpan::new("grandchild")
        .with_attribute("trace_id", child.attributes["trace_id"].clone())
        .with_attribute("parent_span_id", child.attributes["span_id"].clone())
        .with_attribute("span_id", "s-3");

    assert_eq!(grandchild.attributes["trace_id"], "t-1");
    assert_eq!(grandchild.attributes["parent_span_id"], "s-2");
}

#[test]
fn span_serialization_roundtrip() {
    let span = TelemetrySpan::new("serde.op")
        .with_attribute("backend", "mock")
        .with_attribute("dialect", "openai");

    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();

    assert_eq!(span2.name, "serde.op");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["dialect"], "openai");
}

#[test]
fn span_display_via_debug() {
    let span = TelemetrySpan::new("fmt.test").with_attribute("key", "value");
    let dbg = format!("{:?}", span);
    assert!(dbg.contains("fmt.test"));
    assert!(dbg.contains("key"));
}

#[test]
fn span_empty_attributes() {
    let span = TelemetrySpan::new("bare");
    assert!(span.attributes.is_empty());
    let json = serde_json::to_string(&span).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["attributes"], serde_json::json!({}));
}

#[test]
fn span_very_long_name() {
    let long_name = "a".repeat(10_000);
    let span = TelemetrySpan::new(&long_name);
    assert_eq!(span.name.len(), 10_000);

    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, long_name);
}

// =========================================================================
//  2. Metrics types (10 tests)
// =========================================================================

#[test]
fn counter_increment_via_run_summary() {
    let mut s = RunSummary::new();
    s.record_event("tool_call");
    s.record_event("tool_call");
    s.record_event("tool_call");
    assert_eq!(s.tool_call_count, 3);
    assert_eq!(*s.event_counts.get("tool_call").unwrap(), 3);
}

#[test]
fn gauge_via_collector_len() {
    let c = MetricsCollector::new();
    assert_eq!(c.len(), 0);
    c.record(sample("a", 10, 100, 200, 0));
    assert_eq!(c.len(), 1);
    c.record(sample("b", 20, 100, 200, 0));
    assert_eq!(c.len(), 2);
    c.clear();
    assert_eq!(c.len(), 0);
}

#[test]
fn histogram_record_and_stats() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);

    assert_eq!(h.count(), 3);
    assert_eq!(h.min(), Some(10.0));
    assert_eq!(h.max(), Some(30.0));
    assert!((h.mean() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn metric_labels_via_backend_counts() {
    let c = MetricsCollector::new();
    c.record(sample("openai", 100, 10, 20, 0));
    c.record(sample("anthropic", 200, 10, 20, 0));
    c.record(sample("openai", 150, 10, 20, 0));

    let s = c.summary();
    assert_eq!(s.backend_counts["openai"], 2);
    assert_eq!(s.backend_counts["anthropic"], 1);
}

#[test]
fn metric_aggregation_mean_p50_p99() {
    let c = MetricsCollector::new();
    for d in 1u64..=100 {
        c.record(sample("b", d, 10, 20, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 50.5).abs() < f64::EPSILON);
    assert!(s.p50_duration_ms > 49.0 && s.p50_duration_ms < 52.0);
    assert!(s.p99_duration_ms > 98.0 && s.p99_duration_ms <= 100.0);
}

#[test]
fn metric_serialization_run_metrics_roundtrip() {
    let m = RunMetrics {
        backend_name: "serde-test".into(),
        dialect: "openai".into(),
        duration_ms: 1234,
        events_count: 42,
        tokens_in: 500,
        tokens_out: 1000,
        tool_calls_count: 7,
        errors_count: 2,
        emulations_applied: 1,
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn multiple_metrics_registration() {
    let c = MetricsCollector::new();
    for i in 0..10 {
        c.record(RunMetrics {
            backend_name: format!("backend_{}", i % 3),
            dialect: "test".into(),
            duration_ms: (i + 1) * 100,
            events_count: i + 1,
            tokens_in: (i + 1) * 10,
            tokens_out: (i + 1) * 20,
            tool_calls_count: i,
            errors_count: if i % 5 == 0 { 1 } else { 0 },
            emulations_applied: 0,
        });
    }
    let s = c.summary();
    assert_eq!(s.count, 10);
    assert_eq!(s.total_tokens_in, (1..=10).map(|i| i * 10u64).sum::<u64>());
}

#[test]
fn metric_naming_via_run_summary_event_kinds() {
    let mut s = RunSummary::new();
    s.record_event("tool_call");
    s.record_event("error");
    s.record_event("warning");
    s.record_event("custom_metric");

    assert_eq!(s.tool_call_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.warning_count, 1);
    assert_eq!(*s.event_counts.get("custom_metric").unwrap(), 1);
    // custom_metric does not bump error/warning/tool_call
    assert_eq!(s.total_events, 4);
}

#[test]
fn metric_units_cost_estimator() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    // Cost is in USD: 1000 * 0.00003 + 500 * 0.00006 = 0.03 + 0.03 = 0.06
    let cost = ce.estimate("gpt-4", 1000, 500).unwrap();
    assert!((cost - 0.06).abs() < 1e-12);
}

#[test]
fn reset_metrics_via_collector_clear() {
    let c = MetricsCollector::new();
    c.record(sample("a", 100, 50, 60, 1));
    c.record(sample("b", 200, 70, 80, 0));
    assert_eq!(c.len(), 2);

    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.summary(), MetricsSummary::default());

    // Record again after reset.
    c.record(sample("c", 300, 90, 100, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.summary().count, 1);
}

// =========================================================================
//  3. Observability integration (5 tests)
// =========================================================================

#[test]
fn end_to_end_trace_through_pipeline() {
    // Simulate: record runs → build summary → export JSON → verify roundtrip.
    let c = MetricsCollector::new();
    c.record(sample("mock", 100, 500, 1000, 0));
    c.record(sample("mock", 200, 600, 1200, 1));

    let summary = c.summary();
    assert_eq!(summary.count, 2);

    let json = MetricsExporter::export_json(&summary).unwrap();
    let restored: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, summary);

    // Also produce a span referencing the summary.
    let span = TelemetrySpan::new("pipeline.complete")
        .with_attribute("runs", summary.count.to_string())
        .with_attribute("error_rate", format!("{:.2}", summary.error_rate));
    assert_eq!(span.attributes["runs"], "2");
}

#[test]
fn error_traces_include_context() {
    let mut run_summary = RunSummary::new();
    run_summary.record_event("error");
    run_summary.record_event("error");
    run_summary.record_event("tool_call");
    run_summary.set_duration(500);

    assert!(run_summary.has_errors());
    assert_eq!(run_summary.error_count, 2);
    assert!((run_summary.error_rate() - 2.0 / 3.0).abs() < 1e-10);

    // Build a span carrying the error context.
    let span = TelemetrySpan::new("run.with_errors")
        .with_attribute("error_count", run_summary.error_count.to_string())
        .with_attribute("error_rate", format!("{:.4}", run_summary.error_rate()))
        .with_attribute("duration_ms", run_summary.total_duration_ms.to_string());

    assert_eq!(span.attributes["error_count"], "2");
    assert_eq!(span.attributes["duration_ms"], "500");
}

#[test]
fn performance_metrics_for_operations() {
    let mut hist = LatencyHistogram::new();
    for i in 1..=1000 {
        hist.record(i as f64);
    }
    assert_eq!(hist.count(), 1000);
    assert!((hist.mean() - 500.5).abs() < f64::EPSILON);
    assert!(hist.p50() > 499.0 && hist.p50() < 502.0);
    assert!(hist.p95() > 949.0);
    assert!(hist.p99() > 989.0);

    let buckets = hist.buckets(&[100.0, 500.0, 900.0]);
    // [0,100): 99 values, [100,500): 400, [500,900): 400, [900,∞): 101
    assert_eq!(buckets[0], 99);
    assert_eq!(buckets[1], 400);
    assert_eq!(buckets[2], 400);
    assert_eq!(buckets[3], 101);
}

#[test]
fn telemetry_does_not_affect_functionality() {
    // Recording metrics, creating spans, and exporting must not panic or
    // interfere with each other.
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(sample("t", i * 10, 100, 200, 0));
                let _s = cc.summary();
                let span =
                    TelemetrySpan::new(format!("thread.{i}")).with_attribute("iter", i.to_string());
                span.emit(); // must not panic without subscriber
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);

    // Export in all formats without failure.
    let summary = c.summary();
    MetricsExporter::export(&summary, ExportFormat::Json).unwrap();
    MetricsExporter::export(&summary, ExportFormat::Csv).unwrap();
    MetricsExporter::export(&summary, ExportFormat::Structured).unwrap();
}

#[test]
fn telemetry_config_serialization() {
    // ExportFormat and ModelPricing must roundtrip through JSON.
    for fmt in [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Structured,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let fmt2: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, fmt2);
    }

    let pricing = ModelPricing {
        input_cost_per_token: 0.00003,
        output_cost_per_token: 0.00006,
    };
    let json = serde_json::to_string(&pricing).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(pricing, p2);

    // MetricsSummary roundtrip
    let c = MetricsCollector::new();
    c.record(sample("mock", 42, 100, 200, 0));
    let summary = c.summary();
    let json = serde_json::to_string(&summary).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(summary, s2);
}
