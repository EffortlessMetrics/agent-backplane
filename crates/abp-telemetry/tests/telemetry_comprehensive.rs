// Comprehensive telemetry tests — agent-445 / wave-101.
#![allow(clippy::float_cmp)]

use abp_telemetry::*;
use std::collections::BTreeMap;
use std::thread;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run(backend: &str, dialect: &str, dur: u64, tin: u64, tout: u64, errs: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: dialect.into(),
        duration_ms: dur,
        events_count: 1,
        tokens_in: tin,
        tokens_out: tout,
        tool_calls_count: 0,
        errors_count: errs,
        emulations_applied: 0,
    }
}

// =========================================================================
//  1. RunSummary  (15 tests)
// =========================================================================

#[test]
fn run_summary_new_returns_default() {
    let s = RunSummary::new();
    assert_eq!(s, RunSummary::default());
}

#[test]
fn run_summary_from_events_empty_slice() {
    let s = RunSummary::from_events(&[], 500);
    assert_eq!(s.total_events, 0);
    assert_eq!(s.total_duration_ms, 500);
    assert!(s.event_counts.is_empty());
}

#[test]
fn run_summary_from_events_counts_duplicates() {
    let s = RunSummary::from_events(&["tool_call", "tool_call", "error"], 10);
    assert_eq!(*s.event_counts.get("tool_call").unwrap(), 2);
    assert_eq!(*s.event_counts.get("error").unwrap(), 1);
    assert_eq!(s.total_events, 3);
}

#[test]
fn run_summary_record_custom_event_kind() {
    let mut s = RunSummary::new();
    s.record_event("custom_metric");
    assert_eq!(*s.event_counts.get("custom_metric").unwrap(), 1);
    assert_eq!(s.total_events, 1);
    // Custom events don't bump error/warning/tool_call counters.
    assert_eq!(s.error_count, 0);
    assert_eq!(s.warning_count, 0);
    assert_eq!(s.tool_call_count, 0);
}

#[test]
fn run_summary_set_duration_overwrites() {
    let mut s = RunSummary::new();
    s.set_duration(100);
    s.set_duration(999);
    assert_eq!(s.total_duration_ms, 999);
}

#[test]
fn run_summary_error_rate_only_errors() {
    let s = RunSummary::from_events(&["error", "error", "error"], 0);
    assert!((s.error_rate() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn run_summary_has_errors_false_on_empty() {
    assert!(!RunSummary::new().has_errors());
}

#[test]
fn run_summary_merge_overlapping_kinds() {
    let mut a = RunSummary::from_events(&["tool_call", "error"], 100);
    let b = RunSummary::from_events(&["tool_call", "warning"], 200);
    a.merge(&b);
    assert_eq!(*a.event_counts.get("tool_call").unwrap(), 2);
    assert_eq!(*a.event_counts.get("error").unwrap(), 1);
    assert_eq!(*a.event_counts.get("warning").unwrap(), 1);
    assert_eq!(a.total_events, 4);
    assert_eq!(a.total_duration_ms, 300);
    assert_eq!(a.error_count, 1);
    assert_eq!(a.warning_count, 1);
    assert_eq!(a.tool_call_count, 2);
}

#[test]
fn run_summary_merge_into_empty() {
    let mut a = RunSummary::new();
    let b = RunSummary::from_events(&["error"], 50);
    a.merge(&b);
    assert_eq!(a, b);
}

#[test]
fn run_summary_chained_merges() {
    let mut s = RunSummary::new();
    for _ in 0..5 {
        let part = RunSummary::from_events(&["tool_call"], 10);
        s.merge(&part);
    }
    assert_eq!(s.tool_call_count, 5);
    assert_eq!(s.total_duration_ms, 50);
}

#[test]
fn run_summary_serde_preserves_event_counts() {
    let s = RunSummary::from_events(&["error", "tool_call", "info", "info"], 42);
    let json = serde_json::to_string(&s).unwrap();
    let d: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, d);
}

#[test]
fn run_summary_json_keys_alphabetical() {
    let s = RunSummary::from_events(&["z_event", "a_event"], 0);
    let json = serde_json::to_string(&s).unwrap();
    let a_pos = json.find("a_event").unwrap();
    let z_pos = json.find("z_event").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should serialize keys alphabetically");
}

#[test]
fn run_summary_error_rate_zero_events() {
    assert_eq!(RunSummary::new().error_rate(), 0.0);
}

#[test]
fn run_summary_large_event_volume() {
    let mut s = RunSummary::new();
    for _ in 0..10_000 {
        s.record_event("tick");
    }
    assert_eq!(s.total_events, 10_000);
    assert_eq!(*s.event_counts.get("tick").unwrap(), 10_000);
}

#[test]
fn run_summary_clone_is_independent() {
    let mut original = RunSummary::from_events(&["error"], 100);
    let cloned = original.clone();
    original.record_event("tool_call");
    assert_eq!(cloned.total_events, 1);
    assert_eq!(original.total_events, 2);
}

// =========================================================================
//  2. LatencyHistogram  (10 tests)
// =========================================================================

#[test]
fn histogram_empty_percentiles_all_zero() {
    let h = LatencyHistogram::new();
    assert_eq!(h.p50(), 0.0);
    assert_eq!(h.p95(), 0.0);
    assert_eq!(h.p99(), 0.0);
    assert_eq!(h.mean(), 0.0);
}

#[test]
fn histogram_single_value_all_percentiles_equal() {
    let mut h = LatencyHistogram::new();
    h.record(42.0);
    assert_eq!(h.p50(), 42.0);
    assert_eq!(h.p95(), 42.0);
    assert_eq!(h.p99(), 42.0);
    assert_eq!(h.min(), Some(42.0));
    assert_eq!(h.max(), Some(42.0));
}

#[test]
fn histogram_two_values_p50_is_midpoint() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    assert!((h.p50() - 15.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_p95_large_dataset() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    // p95 of 1..=100 via linear interpolation: rank = 0.95 * 99 = 94.05
    assert!(h.p95() > 94.0 && h.p95() < 96.0);
}

#[test]
fn histogram_identical_values() {
    let mut h = LatencyHistogram::new();
    for _ in 0..50 {
        h.record(7.5);
    }
    assert!((h.p50() - 7.5).abs() < f64::EPSILON);
    assert!((h.mean() - 7.5).abs() < f64::EPSILON);
}

#[test]
fn histogram_buckets_empty_boundaries() {
    let mut h = LatencyHistogram::new();
    h.record(5.0);
    h.record(10.0);
    // No boundaries: everything goes in the single overflow bucket.
    let b = h.buckets(&[]);
    assert_eq!(b, vec![2]);
}

#[test]
fn histogram_buckets_fine_grained() {
    let mut h = LatencyHistogram::new();
    for v in [1.0, 5.0, 15.0, 50.0, 150.0] {
        h.record(v);
    }
    let b = h.buckets(&[10.0, 100.0]);
    // [0,10) → 1.0, 5.0 = 2; [10,100) → 15.0, 50.0 = 2; [100,∞) → 150.0 = 1
    assert_eq!(b, vec![2, 2, 1]);
}

#[test]
fn histogram_merge_with_empty() {
    let mut h = LatencyHistogram::new();
    h.record(3.0);
    h.merge(&LatencyHistogram::new());
    assert_eq!(h.count(), 1);
    assert_eq!(h.min(), Some(3.0));
}

#[test]
fn histogram_merge_combines_all_values() {
    let mut a = LatencyHistogram::new();
    let mut b = LatencyHistogram::new();
    a.record(1.0);
    a.record(2.0);
    b.record(3.0);
    b.record(4.0);
    a.merge(&b);
    assert_eq!(a.count(), 4);
    assert_eq!(a.min(), Some(1.0));
    assert_eq!(a.max(), Some(4.0));
}

#[test]
fn histogram_serde_preserves_values() {
    let mut h = LatencyHistogram::new();
    h.record(1.5);
    h.record(2.5);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
    assert_eq!(h2.count(), 2);
}

// =========================================================================
//  3. CostEstimator  (10 tests)
// =========================================================================

#[test]
fn cost_estimator_empty_models_list() {
    assert!(CostEstimator::new().models().is_empty());
}

#[test]
fn cost_estimator_estimate_single_model() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let cost = ce.estimate("gpt-4", 1000, 500).unwrap();
    let expected = 1000.0 * 0.00003 + 500.0 * 0.00006;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn cost_estimator_unknown_model_none() {
    let ce = CostEstimator::new();
    assert!(ce.estimate("nonexistent", 100, 100).is_none());
}

#[test]
fn cost_estimator_zero_tokens_zero_cost() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "model",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    assert_eq!(ce.estimate("model", 0, 0), Some(0.0));
}

#[test]
fn cost_estimator_overwrite_pricing() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 1.0,
            output_cost_per_token: 1.0,
        },
    );
    ce.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 0.5,
            output_cost_per_token: 0.5,
        },
    );
    let cost = ce.estimate("m", 10, 10).unwrap();
    assert!((cost - 10.0).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_total_multiple_models() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "a",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    ce.set_pricing(
        "b",
        ModelPricing {
            input_cost_per_token: 0.003,
            output_cost_per_token: 0.004,
        },
    );
    let total = ce.estimate_total(&[("a", 100, 100), ("b", 100, 100)]);
    let expected = 100.0 * 0.001 + 100.0 * 0.002 + 100.0 * 0.003 + 100.0 * 0.004;
    assert!((total - expected).abs() < 1e-12);
}

#[test]
fn cost_estimator_total_skips_unknown() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "known",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.01,
        },
    );
    let total = ce.estimate_total(&[("known", 10, 10), ("unknown", 999, 999)]);
    let expected = 10.0 * 0.01 + 10.0 * 0.01;
    assert!((total - expected).abs() < 1e-12);
}

#[test]
fn cost_estimator_total_all_unknown_is_zero() {
    let ce = CostEstimator::new();
    assert_eq!(ce.estimate_total(&[("x", 100, 100), ("y", 200, 200)]), 0.0);
}

#[test]
fn cost_estimator_models_list_alphabetical() {
    let mut ce = CostEstimator::new();
    ce.set_pricing("z-model", ModelPricing { input_cost_per_token: 0.0, output_cost_per_token: 0.0 });
    ce.set_pricing("a-model", ModelPricing { input_cost_per_token: 0.0, output_cost_per_token: 0.0 });
    let models = ce.models();
    assert_eq!(models, vec!["a-model", "z-model"]);
}

#[test]
fn cost_estimator_serde_roundtrip_pricing_preserved() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "test",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let json = serde_json::to_string(&ce).unwrap();
    let ce2: CostEstimator = serde_json::from_str(&json).unwrap();
    assert_eq!(
        ce2.estimate("test", 1000, 1000),
        ce.estimate("test", 1000, 1000)
    );
}

// =========================================================================
//  4. MetricsExporter  (10 tests)
// =========================================================================

fn make_summary(count: usize, mean: f64, p50: f64, p99: f64) -> MetricsSummary {
    MetricsSummary {
        count,
        mean_duration_ms: mean,
        p50_duration_ms: p50,
        p99_duration_ms: p99,
        total_tokens_in: 1000,
        total_tokens_out: 2000,
        error_rate: 0.05,
        backend_counts: {
            let mut m = BTreeMap::new();
            m.insert("mock".into(), count);
            m
        },
    }
}

#[test]
fn exporter_json_is_valid_json() {
    let s = make_summary(5, 100.0, 90.0, 200.0);
    let json = MetricsExporter::export_json(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 5);
}

#[test]
fn exporter_json_roundtrip_fidelity() {
    let s = make_summary(3, 50.0, 45.0, 99.0);
    let json = MetricsExporter::export_json(&s).unwrap();
    let d: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, d);
}

#[test]
fn exporter_csv_runs_has_header_and_rows() {
    let runs = vec![
        run("alpha", "openai", 100, 50, 80, 0),
        run("beta", "anthropic", 200, 60, 90, 1),
    ];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3); // header + 2 rows
    assert!(lines[0].starts_with("backend_name,"));
    assert!(lines[1].starts_with("alpha,"));
    assert!(lines[2].starts_with("beta,"));
}

#[test]
fn exporter_csv_empty_runs() {
    let csv = MetricsExporter::export_csv(&[]).unwrap();
    // Should contain only the header line.
    assert_eq!(csv.lines().count(), 1);
}

#[test]
fn exporter_structured_contains_all_keys() {
    let s = make_summary(10, 75.0, 70.0, 150.0);
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count=10"));
    assert!(out.contains("mean_duration_ms=75.00"));
    assert!(out.contains("p50_duration_ms=70.00"));
    assert!(out.contains("p99_duration_ms=150.00"));
    assert!(out.contains("total_tokens_in=1000"));
    assert!(out.contains("total_tokens_out=2000"));
    assert!(out.contains("error_rate=0.0500"));
    assert!(out.contains("backend.mock=10"));
}

#[test]
fn exporter_format_enum_dispatch_json() {
    let s = make_summary(1, 10.0, 10.0, 10.0);
    let out = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    serde_json::from_str::<serde_json::Value>(&out).expect("valid JSON");
}

#[test]
fn exporter_format_enum_dispatch_csv() {
    let s = make_summary(1, 10.0, 10.0, 10.0);
    let csv = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(csv.contains("count,mean_duration_ms"));
}

#[test]
fn exporter_format_enum_dispatch_structured() {
    let s = make_summary(1, 10.0, 10.0, 10.0);
    let out = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(out.contains("count=1"));
}

#[test]
fn exporter_format_serde_roundtrip() {
    for fmt in [ExportFormat::Json, ExportFormat::Csv, ExportFormat::Structured] {
        let json = serde_json::to_string(&fmt).unwrap();
        let d: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(d, fmt);
    }
}

#[test]
fn exporter_json_empty_summary() {
    let s = MetricsSummary::default();
    let json = MetricsExporter::export_json(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 0);
    assert_eq!(v["error_rate"], 0.0);
}

// =========================================================================
//  5. DialectMetrics (per-dialect analysis via collector)  (10 tests)
// =========================================================================

#[test]
fn dialect_metrics_single_dialect_summary() {
    let c = MetricsCollector::new();
    c.record(run("b1", "openai", 100, 50, 60, 0));
    c.record(run("b1", "openai", 200, 70, 80, 1));
    let runs = c.runs();
    let openai: Vec<_> = runs.iter().filter(|r| r.dialect == "openai").collect();
    assert_eq!(openai.len(), 2);
}

#[test]
fn dialect_metrics_mixed_dialects_filter() {
    let c = MetricsCollector::new();
    c.record(run("a", "openai", 10, 1, 1, 0));
    c.record(run("b", "anthropic", 20, 2, 2, 0));
    c.record(run("c", "openai", 30, 3, 3, 0));
    let runs = c.runs();
    let openai_count = runs.iter().filter(|r| r.dialect == "openai").count();
    let anthropic_count = runs.iter().filter(|r| r.dialect == "anthropic").count();
    assert_eq!(openai_count, 2);
    assert_eq!(anthropic_count, 1);
}

#[test]
fn dialect_metrics_success_rate_by_dialect() {
    let c = MetricsCollector::new();
    c.record(run("b", "fast", 10, 1, 1, 0));
    c.record(run("b", "fast", 10, 1, 1, 0));
    c.record(run("b", "slow", 10, 1, 1, 1));
    c.record(run("b", "slow", 10, 1, 1, 1));
    let runs = c.runs();
    let fast_errors: u64 = runs.iter().filter(|r| r.dialect == "fast").map(|r| r.errors_count).sum();
    let slow_errors: u64 = runs.iter().filter(|r| r.dialect == "slow").map(|r| r.errors_count).sum();
    assert_eq!(fast_errors, 0);
    assert_eq!(slow_errors, 2);
}

#[test]
fn dialect_metrics_latency_per_dialect() {
    let c = MetricsCollector::new();
    c.record(run("b", "openai", 100, 1, 1, 0));
    c.record(run("b", "openai", 200, 1, 1, 0));
    c.record(run("b", "anthropic", 50, 1, 1, 0));
    let runs = c.runs();
    let openai_avg: f64 = {
        let ds: Vec<u64> = runs.iter().filter(|r| r.dialect == "openai").map(|r| r.duration_ms).collect();
        ds.iter().sum::<u64>() as f64 / ds.len() as f64
    };
    assert!((openai_avg - 150.0).abs() < f64::EPSILON);
}

#[test]
fn dialect_metrics_token_usage_per_dialect() {
    let c = MetricsCollector::new();
    c.record(run("b", "d1", 10, 100, 200, 0));
    c.record(run("b", "d1", 10, 150, 250, 0));
    c.record(run("b", "d2", 10, 500, 600, 0));
    let runs = c.runs();
    let d1_in: u64 = runs.iter().filter(|r| r.dialect == "d1").map(|r| r.tokens_in).sum();
    let d1_out: u64 = runs.iter().filter(|r| r.dialect == "d1").map(|r| r.tokens_out).sum();
    assert_eq!(d1_in, 250);
    assert_eq!(d1_out, 450);
}

#[test]
fn dialect_metrics_empty_collector_no_dialects() {
    let c = MetricsCollector::new();
    let runs = c.runs();
    let dialects: std::collections::BTreeSet<_> = runs.iter().map(|r| &r.dialect).collect();
    assert!(dialects.is_empty());
}

#[test]
fn dialect_metrics_histogram_from_dialect() {
    let c = MetricsCollector::new();
    c.record(run("b", "fast", 10, 1, 1, 0));
    c.record(run("b", "fast", 30, 1, 1, 0));
    c.record(run("b", "fast", 20, 1, 1, 0));
    let runs = c.runs();
    let mut hist = LatencyHistogram::new();
    for r in runs.iter().filter(|r| r.dialect == "fast") {
        hist.record(r.duration_ms as f64);
    }
    assert_eq!(hist.count(), 3);
    assert!((hist.p50() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn dialect_metrics_separate_collector_per_dialect() {
    let openai_c = MetricsCollector::new();
    let anthropic_c = MetricsCollector::new();
    openai_c.record(run("b", "openai", 100, 10, 20, 0));
    anthropic_c.record(run("b", "anthropic", 200, 30, 40, 1));
    assert_eq!(openai_c.summary().error_rate, 0.0);
    assert_eq!(anthropic_c.summary().error_rate, 1.0);
}

#[test]
fn dialect_metrics_backend_counts_reflect_dialects() {
    let c = MetricsCollector::new();
    c.record(run("backend-a", "d1", 10, 1, 1, 0));
    c.record(run("backend-b", "d1", 10, 1, 1, 0));
    c.record(run("backend-a", "d2", 10, 1, 1, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["backend-a"], 2);
    assert_eq!(s.backend_counts["backend-b"], 1);
}

#[test]
fn dialect_metrics_many_dialects() {
    let c = MetricsCollector::new();
    for i in 0..20 {
        c.record(run("b", &format!("dialect_{}", i), i as u64 * 10, 1, 1, 0));
    }
    let runs = c.runs();
    let unique: std::collections::BTreeSet<_> = runs.iter().map(|r| r.dialect.clone()).collect();
    assert_eq!(unique.len(), 20);
}

// =========================================================================
//  6. Integration  (15 tests)
// =========================================================================

#[test]
fn integration_full_pipeline_record_summarize_export() {
    let c = MetricsCollector::new();
    c.record(run("mock", "openai", 100, 500, 1000, 0));
    c.record(run("mock", "openai", 200, 600, 1200, 1));
    let summary = c.summary();
    let json = MetricsExporter::export_json(&summary).unwrap();
    let parsed: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.count, 2);
    assert_eq!(parsed.total_tokens_in, 1100);
}

#[test]
fn integration_collector_clear_resets_summary() {
    let c = MetricsCollector::new();
    c.record(run("b", "d", 100, 10, 20, 0));
    c.clear();
    assert_eq!(c.summary(), MetricsSummary::default());
}

#[test]
fn integration_collector_record_after_clear() {
    let c = MetricsCollector::new();
    c.record(run("a", "d", 50, 5, 5, 0));
    c.clear();
    c.record(run("b", "d", 100, 10, 10, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.summary().count, 1);
    assert_eq!(c.summary().backend_counts["b"], 1);
}

#[test]
fn integration_concurrent_10_threads_consistent() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(run("b", "d", i * 10, 100, 200, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
    let s = c.summary();
    assert_eq!(s.count, 10);
    assert_eq!(s.total_tokens_in, 1000);
}

#[test]
fn integration_concurrent_summary_read_while_writing() {
    let c = MetricsCollector::new();
    for i in 0..5 {
        c.record(run("b", "d", i * 10, 1, 1, 0));
    }
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                let _ = cc.summary();
                cc.record(run("b", "d", 999, 1, 1, 0));
                cc.summary()
            })
        })
        .collect();
    for h in handles {
        let _ = h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn integration_histogram_from_collector_runs() {
    let c = MetricsCollector::new();
    for d in [50, 100, 150, 200, 250] {
        c.record(run("b", "d", d, 1, 1, 0));
    }
    let mut hist = LatencyHistogram::new();
    for r in c.runs() {
        hist.record(r.duration_ms as f64);
    }
    assert!((hist.p50() - 150.0).abs() < f64::EPSILON);
    assert_eq!(hist.min(), Some(50.0));
    assert_eq!(hist.max(), Some(250.0));
}

#[test]
fn integration_cost_estimation_from_collector() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let c = MetricsCollector::new();
    c.record(run("b", "openai", 100, 500, 1000, 0));
    c.record(run("b", "openai", 200, 300, 800, 0));
    let runs = c.runs();
    let total_in: u64 = runs.iter().map(|r| r.tokens_in).sum();
    let total_out: u64 = runs.iter().map(|r| r.tokens_out).sum();
    let cost = ce.estimate("gpt-4", total_in, total_out).unwrap();
    let expected = 800.0 * 0.00003 + 1800.0 * 0.00006;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn integration_run_summary_from_multiple_runs() {
    let mut combined = RunSummary::new();
    let runs_data: Vec<Vec<&str>> = vec![
        vec!["tool_call", "tool_call", "error"],
        vec!["warning", "tool_call"],
        vec!["error", "error"],
    ];
    for (i, events) in runs_data.iter().enumerate() {
        let s = RunSummary::from_events(events, (i as u64 + 1) * 100);
        combined.merge(&s);
    }
    assert_eq!(combined.total_events, 7);
    assert_eq!(combined.tool_call_count, 3);
    assert_eq!(combined.error_count, 3);
    assert_eq!(combined.warning_count, 1);
    assert_eq!(combined.total_duration_ms, 600);
}

#[test]
fn integration_export_all_formats_same_summary() {
    let c = MetricsCollector::new();
    c.record(run("mock", "test", 100, 500, 1000, 0));
    let s = c.summary();
    let json = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    let csv = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    let structured = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    // JSON should round-trip.
    let restored: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.count, 1);
    // CSV should have header + 1 row.
    assert_eq!(csv.lines().count(), 2);
    // Structured should have key=value lines.
    assert!(structured.contains("count=1"));
}

#[test]
fn integration_json_exporter_trait_object() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 0);
}

#[test]
fn integration_span_with_collector_context() {
    let c = MetricsCollector::new();
    c.record(run("mock", "test", 42, 10, 20, 0));
    let s = c.summary();
    let span = TelemetrySpan::new("run_complete")
        .with_attribute("count", s.count.to_string())
        .with_attribute("mean_ms", format!("{:.2}", s.mean_duration_ms));
    assert_eq!(span.attributes["count"], "1");
    assert_eq!(span.attributes["mean_ms"], "42.00");
}

#[test]
fn integration_histogram_buckets_match_collector() {
    let c = MetricsCollector::new();
    for d in [5, 15, 25, 55, 150, 300] {
        c.record(run("b", "d", d, 1, 1, 0));
    }
    let mut hist = LatencyHistogram::new();
    for r in c.runs() {
        hist.record(r.duration_ms as f64);
    }
    let buckets = hist.buckets(&[10.0, 50.0, 100.0, 200.0]);
    // [0,10)=1  [10,50)=2  [50,100)=1  [100,200)=1  [200,∞)=1
    assert_eq!(buckets, vec![1, 2, 1, 1, 1]);
}

#[test]
fn integration_collector_clone_shares_state() {
    let c1 = MetricsCollector::new();
    let c2 = c1.clone();
    c1.record(run("a", "d", 10, 1, 1, 0));
    c2.record(run("b", "d", 20, 2, 2, 0));
    assert_eq!(c1.len(), 2);
    assert_eq!(c2.len(), 2);
}

#[test]
fn integration_large_scale_summary_stable() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(run("b", "d", i, i, i * 2, if i % 100 == 0 { 1 } else { 0 }));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    // mean of 0..999 = 499.5
    assert!((s.mean_duration_ms - 499.5).abs() < f64::EPSILON);
    assert!(s.p50_duration_ms > 0.0);
    assert!(s.p99_duration_ms > s.p50_duration_ms);
    // 10 errors out of 1000 runs
    assert!((s.error_rate - 0.01).abs() < f64::EPSILON);
}

#[test]
fn integration_concurrent_clear_then_record() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(run("b", "d", 10, 1, 1, 0));
    }
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.clear();
                cc.record(run("b", "d", 20, 2, 2, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // After concurrent clear+record, length should be at least 1 (non-empty).
    assert!(!c.is_empty());
}
