#![allow(clippy::too_many_arguments)]

use abp_telemetry::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_run(
    backend: &str,
    dialect: &str,
    duration_ms: u64,
    tokens_in: u64,
    tokens_out: u64,
    errors: u64,
) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: dialect.to_string(),
        duration_ms,
        events_count: 1,
        tokens_in,
        tokens_out,
        tool_calls_count: 0,
        errors_count: errors,
        emulations_applied: 0,
    }
}

fn simple_run(backend: &str, duration_ms: u64, errors: u64) -> RunMetrics {
    make_run(backend, "test", duration_ms, 100, 200, errors)
}

// =========================================================================
// RunMetrics: construction
// =========================================================================

#[test]
fn run_metrics_default_all_zero() {
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
fn run_metrics_custom_fields() {
    let m = RunMetrics {
        backend_name: "openai".into(),
        dialect: "gpt-4".into(),
        duration_ms: 5000,
        events_count: 42,
        tokens_in: 1024,
        tokens_out: 2048,
        tool_calls_count: 7,
        errors_count: 1,
        emulations_applied: 3,
    };
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
fn run_metrics_clone_is_independent() {
    let m = simple_run("a", 100, 0);
    let mut m2 = m.clone();
    m2.duration_ms = 999;
    assert_eq!(m.duration_ms, 100);
    assert_eq!(m2.duration_ms, 999);
}

#[test]
fn run_metrics_equality() {
    let a = simple_run("x", 50, 1);
    let b = simple_run("x", 50, 1);
    assert_eq!(a, b);
}

#[test]
fn run_metrics_inequality_on_backend() {
    let a = simple_run("x", 50, 0);
    let b = simple_run("y", 50, 0);
    assert_ne!(a, b);
}

#[test]
fn run_metrics_inequality_on_duration() {
    let a = simple_run("x", 50, 0);
    let b = simple_run("x", 51, 0);
    assert_ne!(a, b);
}

// =========================================================================
// RunMetrics: serde roundtrip
// =========================================================================

#[test]
fn run_metrics_serde_roundtrip_full() {
    let m = RunMetrics {
        backend_name: "claude".into(),
        dialect: "claude-3".into(),
        duration_ms: 12345,
        events_count: 10,
        tokens_in: 500,
        tokens_out: 1500,
        tool_calls_count: 4,
        errors_count: 2,
        emulations_applied: 1,
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_serde_roundtrip_default() {
    let m = RunMetrics::default();
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_json_contains_all_fields() {
    let m = simple_run("mock", 100, 0);
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
fn run_metrics_partial_deserialization_fails_missing_field() {
    let json = r#"{"backend_name":"x","dialect":"y"}"#;
    let result = serde_json::from_str::<RunMetrics>(json);
    assert!(result.is_err());
}

#[test]
fn run_metrics_extra_fields_ignored() {
    let json = serde_json::json!({
        "backend_name": "mock",
        "dialect": "test",
        "duration_ms": 10,
        "events_count": 1,
        "tokens_in": 0,
        "tokens_out": 0,
        "tool_calls_count": 0,
        "errors_count": 0,
        "emulations_applied": 0,
        "extra_field": "should be ignored"
    });
    let m: RunMetrics = serde_json::from_value(json).unwrap();
    assert_eq!(m.backend_name, "mock");
}

#[test]
fn run_metrics_debug_impl() {
    let m = simple_run("dbg", 1, 0);
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("dbg"));
}

// =========================================================================
// RunMetrics: edge cases
// =========================================================================

#[test]
fn run_metrics_zero_duration() {
    let m = simple_run("z", 0, 0);
    assert_eq!(m.duration_ms, 0);
}

#[test]
fn run_metrics_max_u64_tokens() {
    let m = RunMetrics {
        tokens_in: u64::MAX,
        tokens_out: u64::MAX,
        ..RunMetrics::default()
    };
    assert_eq!(m.tokens_in, u64::MAX);
    assert_eq!(m.tokens_out, u64::MAX);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.tokens_in, u64::MAX);
}

#[test]
fn run_metrics_max_u64_duration() {
    let m = RunMetrics {
        duration_ms: u64::MAX,
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.duration_ms, u64::MAX);
}

#[test]
fn run_metrics_unicode_backend_name() {
    let m = RunMetrics {
        backend_name: "бэкенд-🚀".into(),
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.backend_name, "бэкенд-🚀");
}

#[test]
fn run_metrics_empty_strings() {
    let m = RunMetrics::default();
    assert!(m.backend_name.is_empty());
    assert!(m.dialect.is_empty());
}

// =========================================================================
// MetricsCollector: basic operations
// =========================================================================

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
    c.record(simple_run("a", 10, 0));
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
    c.record(simple_run("b", 20, 0));
    assert_eq!(c.len(), 2);
}

#[test]
fn collector_runs_returns_in_order() {
    let c = MetricsCollector::new();
    c.record(simple_run("first", 10, 0));
    c.record(simple_run("second", 20, 0));
    c.record(simple_run("third", 30, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].backend_name, "first");
    assert_eq!(runs[1].backend_name, "second");
    assert_eq!(runs[2].backend_name, "third");
}

#[test]
fn collector_runs_returns_clone() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 1);
    // Recording more doesn't affect previous snapshot
    c.record(simple_run("b", 20, 0));
    assert_eq!(runs.len(), 1);
    assert_eq!(c.len(), 2);
}

#[test]
fn collector_clear_empties() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("b", 20, 0));
    assert_eq!(c.len(), 2);
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert!(c.runs().is_empty());
}

#[test]
fn collector_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    c.clear();
    c.record(simple_run("b", 20, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

#[test]
fn collector_clone_shares_state() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    c.record(simple_run("shared", 10, 0));
    // Because inner is Arc<Mutex<...>>, clone shares state
    assert_eq!(c2.len(), 1);
    assert_eq!(c2.runs()[0].backend_name, "shared");
}

#[test]
fn collector_clone_bidirectional() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    c.record(simple_run("from_c", 10, 0));
    c2.record(simple_run("from_c2", 20, 0));
    assert_eq!(c.len(), 2);
    assert_eq!(c2.len(), 2);
}

#[test]
fn collector_debug_impl() {
    let c = MetricsCollector::new();
    let dbg = format!("{:?}", c);
    assert!(!dbg.is_empty());
}

// =========================================================================
// MetricsCollector: summary on empty
// =========================================================================

#[test]
fn empty_summary_all_defaults() {
    let s = MetricsCollector::new().summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.p50_duration_ms, 0.0);
    assert_eq!(s.p99_duration_ms, 0.0);
    assert_eq!(s.total_tokens_in, 0);
    assert_eq!(s.total_tokens_out, 0);
    assert_eq!(s.error_rate, 0.0);
    assert!(s.backend_counts.is_empty());
}

// =========================================================================
// MetricsSummary: math correctness
// =========================================================================

#[test]
fn summary_single_run_all_fields() {
    let c = MetricsCollector::new();
    c.record(make_run("mock", "d", 42, 500, 1000, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
    assert_eq!(s.total_tokens_in, 500);
    assert_eq!(s.total_tokens_out, 1000);
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["mock"], 1);
}

#[test]
fn summary_mean_two_runs() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("a", 200, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_mean_three_runs() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("a", 200, 0));
    c.record(simple_run("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_odd_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_even_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_two_items() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("a", 200, 0));
    let s = c.summary();
    assert!((s.p50_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p99_hundred_runs() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn summary_p99_two_items() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("a", 1000, 0));
    let s = c.summary();
    // p99 of [10, 1000] with rank = 0.99 * 1 = 0.99
    // interpolation: 10 * 0.01 + 1000 * 0.99 = 990.1
    assert!((s.p99_duration_ms - 990.1).abs() < 0.01);
}

#[test]
fn summary_token_aggregation() {
    let c = MetricsCollector::new();
    c.record(make_run("a", "d", 10, 100, 200, 0));
    c.record(make_run("b", "d", 20, 300, 400, 0));
    c.record(make_run("c", "d", 30, 500, 600, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 900);
    assert_eq!(s.total_tokens_out, 1200);
}

#[test]
fn summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(simple_run("a", 10, 0));
    }
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(simple_run("a", 10, 1));
    }
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_mixed() {
    let c = MetricsCollector::new();
    // 4 runs: errors = 0, 2, 0, 2 => total 4, rate = 4/4 = 1.0
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("a", 10, 2));
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("a", 10, 2));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_half() {
    let c = MetricsCollector::new();
    // 4 runs each with 1 error => 4 errors / 4 runs = 1.0
    // For half: 2 runs with 1 error, 2 with 0 => 2/4 = 0.5
    c.record(simple_run("a", 10, 1));
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("a", 10, 1));
    c.record(simple_run("a", 10, 0));
    let s = c.summary();
    assert!((s.error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn summary_backend_counts_single() {
    let c = MetricsCollector::new();
    c.record(simple_run("only", 10, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["only"], 1);
}

#[test]
fn summary_backend_counts_multiple() {
    let c = MetricsCollector::new();
    c.record(simple_run("alpha", 10, 0));
    c.record(simple_run("beta", 20, 0));
    c.record(simple_run("alpha", 30, 0));
    c.record(simple_run("gamma", 40, 0));
    c.record(simple_run("beta", 50, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 3);
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 2);
    assert_eq!(s.backend_counts["gamma"], 1);
}

#[test]
fn summary_backend_counts_deterministic_order() {
    let c = MetricsCollector::new();
    c.record(simple_run("zebra", 10, 0));
    c.record(simple_run("alpha", 20, 0));
    c.record(simple_run("middle", 30, 0));
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

// =========================================================================
// MetricsSummary: serde roundtrip
// =========================================================================

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 50, 1));
    c.record(simple_run("b", 100, 0));
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
fn metrics_summary_json_has_all_fields() {
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
fn metrics_summary_clone() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 1));
    let s = c.summary();
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_debug_impl() {
    let s = MetricsSummary::default();
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("MetricsSummary"));
}

// =========================================================================
// MetricsSummary: large data sets
// =========================================================================

#[test]
fn summary_thousand_runs() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(simple_run("bulk", i, 0));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    // Mean of 0..999 = 499.5
    assert!((s.mean_duration_ms - 499.5).abs() < 0.01);
    assert_eq!(s.total_tokens_in, 100 * 1000);
    assert_eq!(s.total_tokens_out, 200 * 1000);
    assert_eq!(s.backend_counts["bulk"], 1000);
}

#[test]
fn summary_five_thousand_runs() {
    let c = MetricsCollector::new();
    for i in 0..5000 {
        c.record(simple_run("mass", i % 100, if i % 10 == 0 { 1 } else { 0 }));
    }
    let s = c.summary();
    assert_eq!(s.count, 5000);
    // 500 errors out of 5000 = 0.1
    assert!((s.error_rate - 0.1).abs() < f64::EPSILON);
}

#[test]
fn summary_all_same_duration() {
    let c = MetricsCollector::new();
    for _ in 0..100 {
        c.record(simple_run("same", 42, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_zero_duration_runs() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(simple_run("zero", 0, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 0.0).abs() < f64::EPSILON);
}

// =========================================================================
// MetricsSummary: property-style tests
// =========================================================================

#[test]
fn property_mean_is_between_min_and_max() {
    let c = MetricsCollector::new();
    let durations = [5, 10, 15, 20, 100, 200, 500];
    for &d in &durations {
        c.record(simple_run("p", d, 0));
    }
    let s = c.summary();
    let min = *durations.iter().min().unwrap() as f64;
    let max = *durations.iter().max().unwrap() as f64;
    assert!(s.mean_duration_ms >= min);
    assert!(s.mean_duration_ms <= max);
}

#[test]
fn property_p50_leq_p99() {
    let c = MetricsCollector::new();
    for d in [1, 2, 3, 10, 50, 100, 500, 1000, 5000, 10000] {
        c.record(simple_run("p", d, 0));
    }
    let s = c.summary();
    assert!(s.p50_duration_ms <= s.p99_duration_ms);
}

#[test]
fn property_total_tokens_equal_sum() {
    let c = MetricsCollector::new();
    let mut expected_in = 0u64;
    let mut expected_out = 0u64;
    for i in 0..50 {
        let tin = i * 10;
        let tout = i * 20;
        c.record(make_run("t", "d", 1, tin, tout, 0));
        expected_in += tin;
        expected_out += tout;
    }
    let s = c.summary();
    assert_eq!(s.total_tokens_in, expected_in);
    assert_eq!(s.total_tokens_out, expected_out);
}

#[test]
fn property_error_rate_bounded_0_to_max() {
    let c = MetricsCollector::new();
    // Each run can have multiple errors so error_rate can exceed 1.0
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("a", 10, 3));
    let s = c.summary();
    assert!(s.error_rate >= 0.0);
    // 3 errors / 2 runs = 1.5
    assert!((s.error_rate - 1.5).abs() < f64::EPSILON);
}

#[test]
fn property_count_matches_recorded() {
    let c = MetricsCollector::new();
    let n = 73;
    for i in 0..n {
        c.record(simple_run("cnt", i, 0));
    }
    let s = c.summary();
    assert_eq!(s.count, n as usize);
}

#[test]
fn property_backend_counts_sum_to_total() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    c.record(simple_run("b", 20, 0));
    c.record(simple_run("a", 30, 0));
    c.record(simple_run("c", 40, 0));
    c.record(simple_run("b", 50, 0));
    let s = c.summary();
    let sum: usize = s.backend_counts.values().sum();
    assert_eq!(sum, s.count);
}

#[test]
fn property_mean_statistical_loop() {
    // Verify mean over sequential integers 0..N = (N-1)/2
    for n in [2, 5, 10, 50, 100] {
        let c = MetricsCollector::new();
        for i in 0..n {
            c.record(simple_run("stat", i, 0));
        }
        let s = c.summary();
        let expected = (n - 1) as f64 / 2.0;
        assert!(
            (s.mean_duration_ms - expected).abs() < 0.01,
            "n={n}: expected mean {expected}, got {}",
            s.mean_duration_ms
        );
    }
}

#[test]
fn property_p50_odd_count_loop() {
    // For sorted 0..(2k+1), median = k
    for k in [1, 2, 5, 10, 25] {
        let n = 2 * k + 1;
        let c = MetricsCollector::new();
        for i in 0..n {
            c.record(simple_run("med", i as u64, 0));
        }
        let s = c.summary();
        let expected = k as f64;
        assert!(
            (s.p50_duration_ms - expected).abs() < 0.01,
            "k={k}: expected p50 {expected}, got {}",
            s.p50_duration_ms
        );
    }
}

// =========================================================================
// TelemetrySpan: construction
// =========================================================================

#[test]
fn span_new_name() {
    let span = TelemetrySpan::new("test_op");
    assert_eq!(span.name, "test_op");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_new_from_string() {
    let span = TelemetrySpan::new(String::from("owned_name"));
    assert_eq!(span.name, "owned_name");
}

#[test]
fn span_with_single_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("key", "val");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "val");
}

#[test]
fn span_chaining_multiple_attributes() {
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
fn span_attribute_overwrites_same_key() {
    let span = TelemetrySpan::new("op")
        .with_attribute("key", "first")
        .with_attribute("key", "second");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "second");
}

#[test]
fn span_empty_name() {
    let span = TelemetrySpan::new("");
    assert!(span.name.is_empty());
}

#[test]
fn span_empty_attribute_key_and_value() {
    let span = TelemetrySpan::new("op").with_attribute("", "");
    assert!(span.attributes.contains_key(""));
    assert_eq!(span.attributes[""], "");
}

#[test]
fn span_clone() {
    let span = TelemetrySpan::new("op").with_attribute("k", "v");
    let span2 = span.clone();
    assert_eq!(span.name, span2.name);
    assert_eq!(span.attributes, span2.attributes);
}

#[test]
fn span_debug_impl() {
    let span = TelemetrySpan::new("dbg_op");
    let dbg = format!("{:?}", span);
    assert!(dbg.contains("dbg_op"));
}

#[test]
fn span_unicode_name_and_attributes() {
    let span = TelemetrySpan::new("操作")
        .with_attribute("ключ", "значение")
        .with_attribute("emoji", "🎯");
    assert_eq!(span.name, "操作");
    assert_eq!(span.attributes["ключ"], "значение");
    assert_eq!(span.attributes["emoji"], "🎯");
}

// =========================================================================
// TelemetrySpan: serde roundtrip
// =========================================================================

#[test]
fn span_serde_roundtrip_no_attrs() {
    let span = TelemetrySpan::new("bare");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "bare");
    assert!(span2.attributes.is_empty());
}

#[test]
fn span_serde_roundtrip_with_attrs() {
    let span = TelemetrySpan::new("run")
        .with_attribute("backend", "mock")
        .with_attribute("mode", "mapped");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes.len(), 2);
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["mode"], "mapped");
}

#[test]
fn span_json_attributes_deterministic_order() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z_last", "1")
        .with_attribute("a_first", "2");
    let json = serde_json::to_string(&span).unwrap();
    let a_pos = json.find("a_first").unwrap();
    let z_pos = json.find("z_last").unwrap();
    assert!(
        a_pos < z_pos,
        "BTreeMap should serialize in alphabetical order"
    );
}

// =========================================================================
// TelemetrySpan: emit (doesn't panic)
// =========================================================================

#[test]
fn span_emit_does_not_panic() {
    let span = TelemetrySpan::new("safe_op").with_attribute("test", "true");
    span.emit();
}

#[test]
fn span_emit_empty_does_not_panic() {
    let span = TelemetrySpan::new("");
    span.emit();
}

#[test]
fn span_emit_many_attributes_does_not_panic() {
    let mut span = TelemetrySpan::new("big");
    for i in 0..100 {
        span = span.with_attribute(format!("key_{i}"), format!("val_{i}"));
    }
    span.emit();
}

// =========================================================================
// JsonExporter: export valid JSON
// =========================================================================

#[test]
fn json_exporter_returns_valid_json() {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn json_exporter_pretty_printed() {
    let s = MetricsSummary::default();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    // Pretty-printed JSON has newlines
    assert!(json.contains('\n'));
}

#[test]
fn json_exporter_empty_summary() {
    let s = MetricsSummary::default();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
    assert_eq!(parsed["mean_duration_ms"], 0.0);
    assert_eq!(parsed["total_tokens_in"], 0);
    assert_eq!(parsed["total_tokens_out"], 0);
}

#[test]
fn json_exporter_deterministic_output() {
    let c = MetricsCollector::new();
    c.record(simple_run("zebra", 10, 0));
    c.record(simple_run("alpha", 20, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json1 = exporter.export(&s).unwrap();
    let json2 = exporter.export(&s).unwrap();
    assert_eq!(json1, json2, "Repeated exports should be identical");
}

#[test]
fn json_exporter_backend_order() {
    let c = MetricsCollector::new();
    c.record(simple_run("zebra", 10, 0));
    c.record(simple_run("alpha", 20, 0));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < zebra_pos);
}

#[test]
fn json_exporter_all_fields_present() {
    let c = MetricsCollector::new();
    c.record(simple_run("x", 100, 1));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("count").is_some());
    assert!(v.get("mean_duration_ms").is_some());
    assert!(v.get("p50_duration_ms").is_some());
    assert!(v.get("p99_duration_ms").is_some());
    assert!(v.get("total_tokens_in").is_some());
    assert!(v.get("total_tokens_out").is_some());
    assert!(v.get("error_rate").is_some());
    assert!(v.get("backend_counts").is_some());
}

#[test]
fn json_exporter_numeric_types() {
    let c = MetricsCollector::new();
    c.record(simple_run("num", 42, 1));
    let s = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["count"].is_number());
    assert!(v["mean_duration_ms"].is_number());
    assert!(v["total_tokens_in"].is_number());
    assert!(v["error_rate"].is_number());
}

#[test]
fn json_exporter_as_trait_object() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let result = exporter.export(&s);
    assert!(result.is_ok());
}

// =========================================================================
// TelemetryExporter: trait object usage
// =========================================================================

#[test]
fn exporter_trait_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<JsonExporter>();
}

#[test]
fn exporter_trait_arc_sharing() {
    let exporter: Arc<dyn TelemetryExporter> = Arc::new(JsonExporter);
    let s = MetricsSummary::default();
    let e1 = exporter.clone();
    let e2 = exporter.clone();
    let r1 = e1.export(&s).unwrap();
    let r2 = e2.export(&s).unwrap();
    assert_eq!(r1, r2);
}

// =========================================================================
// Thread safety stress tests
// =========================================================================

#[test]
fn concurrent_recording_10_threads() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(simple_run("thread", i * 10, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn concurrent_recording_100_threads() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..100)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(simple_run("stress", i, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 100);
}

#[test]
fn concurrent_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(simple_run("pre", 10, 0));
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(simple_run("t", i, 0));
                let _ = cc.summary();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 21);
}

#[test]
fn concurrent_reads_and_writes() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..50)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                if i % 2 == 0 {
                    cc.record(simple_run("w", i, 0));
                } else {
                    let _ = cc.runs();
                    let _ = cc.len();
                    let _ = cc.is_empty();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // 25 writes (even indices 0,2,4,...,48)
    assert_eq!(c.len(), 25);
}

#[test]
fn concurrent_clear_and_record() {
    let c = MetricsCollector::new();
    for _ in 0..100 {
        c.record(simple_run("init", 1, 0));
    }
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                if i == 0 {
                    cc.clear();
                } else {
                    cc.record(simple_run("post", 1, 0));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // Non-deterministic outcome but should not panic
    let _ = c.len();
}

#[test]
fn concurrent_export_with_arc() {
    let c = MetricsCollector::new();
    for i in 0..10 {
        c.record(simple_run("exp", i, 0));
    }
    let summary = c.summary();
    let exporter: Arc<dyn TelemetryExporter> = Arc::new(JsonExporter);
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let e = exporter.clone();
            let s = summary.clone();
            thread::spawn(move || {
                let result = e.export(&s);
                assert!(result.is_ok());
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// =========================================================================
// Edge cases
// =========================================================================

#[test]
fn collector_many_backends() {
    let c = MetricsCollector::new();
    for i in 0..100 {
        c.record(simple_run(&format!("backend_{i}"), i, 0));
    }
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 100);
}

#[test]
fn summary_after_clear_is_default() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 1));
    c.clear();
    let s = c.summary();
    assert_eq!(s, MetricsSummary::default());
}

#[test]
fn run_metrics_all_fields_max() {
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
fn summary_with_only_errors() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(simple_run("fail", 100, 5));
    }
    let s = c.summary();
    // 50 errors / 10 runs = 5.0
    assert!((s.error_rate - 5.0).abs() < f64::EPSILON);
}

#[test]
fn summary_durations_not_affected_by_insertion_order() {
    // Insert in reverse order; summary should still compute correctly
    let c = MetricsCollector::new();
    for d in (0..10).rev() {
        c.record(simple_run("rev", d, 0));
    }
    let s = c.summary();
    // mean of 0..9 = 4.5
    assert!((s.mean_duration_ms - 4.5).abs() < 0.01);
}

#[test]
fn span_with_many_attributes() {
    let mut span = TelemetrySpan::new("big");
    for i in 0..1000 {
        span = span.with_attribute(format!("k{i}"), format!("v{i}"));
    }
    assert_eq!(span.attributes.len(), 1000);
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.attributes.len(), 1000);
}

#[test]
fn metrics_summary_manual_construction() {
    let mut bc = BTreeMap::new();
    bc.insert("test".to_string(), 5);
    let s = MetricsSummary {
        count: 5,
        mean_duration_ms: 100.0,
        p50_duration_ms: 90.0,
        p99_duration_ms: 200.0,
        total_tokens_in: 1000,
        total_tokens_out: 2000,
        error_rate: 0.2,
        backend_counts: bc,
    };
    assert_eq!(s.count, 5);
    assert_eq!(s.backend_counts["test"], 5);
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn json_exporter_debug_impl() {
    let e = JsonExporter;
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("JsonExporter"));
}

#[test]
fn collector_record_after_summary() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 10, 0));
    let s1 = c.summary();
    c.record(simple_run("b", 20, 0));
    let s2 = c.summary();
    assert_eq!(s1.count, 1);
    assert_eq!(s2.count, 2);
}

#[test]
fn collector_multiple_summaries_consistent() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 50, 1));
    let s1 = c.summary();
    let s2 = c.summary();
    assert_eq!(s1, s2);
}
