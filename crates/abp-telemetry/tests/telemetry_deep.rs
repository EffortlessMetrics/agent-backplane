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

// ===========================================================================
// RunSummary tests (15)
// ===========================================================================

#[test]
fn run_summary_default_is_empty() {
    let s = RunSummary::new();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.error_count, 0);
    assert_eq!(s.warning_count, 0);
    assert_eq!(s.tool_call_count, 0);
    assert_eq!(s.total_duration_ms, 0);
    assert!(s.event_counts.is_empty());
}

#[test]
fn run_summary_record_single_event() {
    let mut s = RunSummary::new();
    s.record_event("assistant_message");
    assert_eq!(s.total_events, 1);
    assert_eq!(s.event_counts["assistant_message"], 1);
}

#[test]
fn run_summary_record_multiple_same_kind() {
    let mut s = RunSummary::new();
    s.record_event("assistant_delta");
    s.record_event("assistant_delta");
    s.record_event("assistant_delta");
    assert_eq!(s.total_events, 3);
    assert_eq!(s.event_counts["assistant_delta"], 3);
}

#[test]
fn run_summary_record_error_increments_error_count() {
    let mut s = RunSummary::new();
    s.record_event("error");
    s.record_event("error");
    assert_eq!(s.error_count, 2);
    assert_eq!(s.total_events, 2);
}

#[test]
fn run_summary_record_warning_increments_warning_count() {
    let mut s = RunSummary::new();
    s.record_event("warning");
    assert_eq!(s.warning_count, 1);
}

#[test]
fn run_summary_record_tool_call_increments_count() {
    let mut s = RunSummary::new();
    s.record_event("tool_call");
    s.record_event("tool_call");
    s.record_event("tool_call");
    assert_eq!(s.tool_call_count, 3);
}

#[test]
fn run_summary_set_duration() {
    let mut s = RunSummary::new();
    s.set_duration(1500);
    assert_eq!(s.total_duration_ms, 1500);
}

#[test]
fn run_summary_has_errors() {
    let mut s = RunSummary::new();
    assert!(!s.has_errors());
    s.record_event("error");
    assert!(s.has_errors());
}

#[test]
fn run_summary_error_rate_no_events() {
    let s = RunSummary::new();
    assert_eq!(s.error_rate(), 0.0);
}

#[test]
fn run_summary_error_rate_mixed() {
    let mut s = RunSummary::new();
    s.record_event("error");
    s.record_event("assistant_message");
    s.record_event("tool_call");
    s.record_event("error");
    // 2 errors out of 4 events = 0.5
    assert!((s.error_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn run_summary_merge_two_summaries() {
    let mut s1 = RunSummary::from_events(&["error", "tool_call"], 100);
    let s2 = RunSummary::from_events(&["warning", "tool_call", "tool_call"], 200);
    s1.merge(&s2);
    assert_eq!(s1.total_events, 5);
    assert_eq!(s1.error_count, 1);
    assert_eq!(s1.warning_count, 1);
    assert_eq!(s1.tool_call_count, 3);
    assert_eq!(s1.total_duration_ms, 300);
}

#[test]
fn run_summary_merge_empty_into_populated() {
    let mut s1 = RunSummary::from_events(&["error"], 50);
    let s2 = RunSummary::new();
    s1.merge(&s2);
    assert_eq!(s1.total_events, 1);
    assert_eq!(s1.total_duration_ms, 50);
}

#[test]
fn run_summary_from_events_builds_correctly() {
    let s = RunSummary::from_events(
        &[
            "run_started",
            "tool_call",
            "tool_result",
            "error",
            "run_completed",
        ],
        500,
    );
    assert_eq!(s.total_events, 5);
    assert_eq!(s.event_counts["run_started"], 1);
    assert_eq!(s.event_counts["tool_call"], 1);
    assert_eq!(s.tool_call_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.total_duration_ms, 500);
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["error", "tool_call", "assistant_delta"], 999);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn run_summary_event_counts_deterministic_order() {
    let mut s = RunSummary::new();
    s.record_event("zebra");
    s.record_event("alpha");
    s.record_event("middle");
    let keys: Vec<&String> = s.event_counts.keys().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

// ===========================================================================
// LatencyHistogram tests (10)
// ===========================================================================

#[test]
fn histogram_new_is_empty() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.count(), 0);
    assert_eq!(h.mean(), 0.0);
}

#[test]
fn histogram_record_and_count() {
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
    h.record(100.0);
    h.record(50.0);
    assert_eq!(h.min(), Some(5.0));
    assert_eq!(h.max(), Some(100.0));
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
fn histogram_p50_odd() {
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
    assert!(h.p95() > 94.0 && h.p95() <= 96.0);
    assert!(h.p99() > 98.0 && h.p99() <= 100.0);
}

#[test]
fn histogram_merge() {
    let mut h1 = LatencyHistogram::new();
    h1.record(10.0);
    h1.record(20.0);
    let mut h2 = LatencyHistogram::new();
    h2.record(30.0);
    h1.merge(&h2);
    assert_eq!(h1.count(), 3);
    assert!((h1.mean() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_buckets() {
    let mut h = LatencyHistogram::new();
    for v in [5.0, 15.0, 25.0, 35.0, 150.0] {
        h.record(v);
    }
    let b = h.buckets(&[10.0, 20.0, 50.0, 100.0]);
    // [0,10): 5.0 → 1
    // [10,20): 15.0 → 1
    // [20,50): 25.0, 35.0 → 2
    // [50,100): → 0
    // [100,∞): 150.0 → 1
    assert_eq!(b, vec![1, 1, 2, 0, 1]);
}

#[test]
fn histogram_serde_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(1.5);
    h.record(2.5);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
}

// ===========================================================================
// CostEstimator tests (10)
// ===========================================================================

#[test]
fn cost_estimator_new_is_empty() {
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
    let p = e.get_pricing("gpt-4").unwrap();
    assert!((p.input_cost_per_token - 0.00003).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_unknown_model_returns_none() {
    let e = CostEstimator::new();
    assert!(e.estimate("unknown", 100, 100).is_none());
}

#[test]
fn cost_estimator_single_model() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let cost = e.estimate("gpt-4", 1000, 500).unwrap();
    let expected = 1000.0 * 0.00003 + 500.0 * 0.00006;
    assert!((cost - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_zero_tokens() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "model",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    assert_eq!(e.estimate("model", 0, 0), Some(0.0));
}

#[test]
fn cost_estimator_multi_model_total() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    e.set_pricing(
        "claude",
        ModelPricing {
            input_cost_per_token: 0.00001,
            output_cost_per_token: 0.00003,
        },
    );
    let total = e.estimate_total(&[("gpt-4", 1000, 500), ("claude", 2000, 1000)]);
    let expected = (1000.0 * 0.00003 + 500.0 * 0.00006) + (2000.0 * 0.00001 + 1000.0 * 0.00003);
    assert!((total - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_skips_unknown_in_total() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let total = e.estimate_total(&[("gpt-4", 1000, 0), ("unknown", 5000, 5000)]);
    let expected = 1000.0 * 0.00003;
    assert!((total - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_overwrite_pricing() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "model",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    e.set_pricing(
        "model",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    let cost = e.estimate("model", 100, 100).unwrap();
    let expected = 100.0 * 0.01 + 100.0 * 0.02;
    assert!((cost - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_models_list() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "beta",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    e.set_pricing(
        "alpha",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    let models = e.models();
    // BTreeMap ensures alphabetical order
    assert_eq!(models, vec!["alpha", "beta"]);
}

#[test]
fn cost_estimator_serde_roundtrip() {
    let mut e = CostEstimator::new();
    e.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let json = serde_json::to_string(&e).unwrap();
    let e2: CostEstimator = serde_json::from_str(&json).unwrap();
    assert_eq!(
        e2.estimate("gpt-4", 1000, 500),
        e.estimate("gpt-4", 1000, 500)
    );
}

// ===========================================================================
// MetricsExporter tests (10)
// ===========================================================================

fn make_summary_for_export() -> MetricsSummary {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 1));
    c.record(simple_run("sidecar", 200, 0));
    c.summary()
}

#[test]
fn exporter_json_valid() {
    let s = make_summary_for_export();
    let json = MetricsExporter::export_json(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 2);
}

#[test]
fn exporter_json_roundtrip() {
    let s = make_summary_for_export();
    let json = MetricsExporter::export_json(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn exporter_csv_runs_header() {
    let runs = vec![simple_run("mock", 100, 0)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    assert!(csv.starts_with("backend_name,"));
    assert!(csv.contains("mock"));
}

#[test]
fn exporter_csv_runs_row_count() {
    let runs = vec![
        simple_run("a", 10, 0),
        simple_run("b", 20, 1),
        simple_run("c", 30, 0),
    ];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 4); // header + 3 rows
}

#[test]
fn exporter_csv_empty_runs() {
    let csv = MetricsExporter::export_csv(&[]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 1); // header only
}

#[test]
fn exporter_structured_format() {
    let s = make_summary_for_export();
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count=2"));
    assert!(out.contains("total_tokens_in="));
    assert!(out.contains("backend.mock=1"));
}

#[test]
fn exporter_format_enum_json() {
    let s = make_summary_for_export();
    let out = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    let _: serde_json::Value = serde_json::from_str(&out).unwrap();
}

#[test]
fn exporter_format_enum_csv() {
    let s = make_summary_for_export();
    let out = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(out.contains("count,"));
    assert!(out.lines().count() >= 2);
}

#[test]
fn exporter_format_enum_structured() {
    let s = make_summary_for_export();
    let out = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(out.contains("count="));
    assert!(out.contains("error_rate="));
}

#[test]
fn exporter_empty_summary_all_formats() {
    let s = MetricsSummary::default();
    assert!(MetricsExporter::export(&s, ExportFormat::Json).is_ok());
    assert!(MetricsExporter::export(&s, ExportFormat::Csv).is_ok());
    assert!(MetricsExporter::export(&s, ExportFormat::Structured).is_ok());
}

// ===========================================================================
// Integration tests (15)
// ===========================================================================

#[test]
fn integration_events_to_summary() {
    let events = [
        "run_started",
        "assistant_delta",
        "assistant_delta",
        "tool_call",
        "tool_result",
        "assistant_message",
        "error",
        "run_completed",
    ];
    let s = RunSummary::from_events(&events, 1200);
    assert_eq!(s.total_events, 8);
    assert_eq!(s.tool_call_count, 1);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.total_duration_ms, 1200);
    assert!((s.error_rate() - 0.125).abs() < f64::EPSILON);
}

#[test]
fn integration_summary_to_json_export() {
    let s = RunSummary::from_events(&["tool_call", "error"], 500);
    let json = serde_json::to_string(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["total_events"], 2);
    assert_eq!(parsed["error_count"], 1);
}

#[test]
fn integration_collector_to_exporter_pipeline() {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 0));
    c.record(simple_run("mock", 200, 1));
    let summary = c.summary();
    let json = MetricsExporter::export_json(&summary).unwrap();
    let parsed: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.count, 2);
    assert_eq!(parsed.backend_counts["mock"], 2);
}

#[test]
fn integration_histogram_with_run_durations() {
    let c = MetricsCollector::new();
    let mut h = LatencyHistogram::new();
    for d in [50, 100, 150, 200, 250] {
        c.record(simple_run("mock", d, 0));
        h.record(d as f64);
    }
    assert_eq!(c.len(), 5);
    assert!((h.p50() - 150.0).abs() < f64::EPSILON);
    assert!(h.p95() > 200.0);
}

#[test]
fn integration_cost_estimation_from_runs() {
    let mut estimator = CostEstimator::new();
    estimator.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );

    let runs = [simple_run("gpt-4", 100, 0), simple_run("gpt-4", 200, 0)];
    let usages: Vec<(&str, u64, u64)> = runs
        .iter()
        .map(|r| ("gpt-4", r.tokens_in, r.tokens_out))
        .collect();
    let total_cost = estimator.estimate_total(&usages);
    assert!(total_cost > 0.0);
}

#[test]
fn integration_full_pipeline_collect_summarize_export() {
    // 1. Collect
    let c = MetricsCollector::new();
    for i in 0..10 {
        c.record(simple_run(
            "backend",
            i * 100,
            if i % 3 == 0 { 1 } else { 0 },
        ));
    }

    // 2. Summarize
    let summary = c.summary();
    assert_eq!(summary.count, 10);

    // 3. Export all formats
    let json = MetricsExporter::export(&summary, ExportFormat::Json).unwrap();
    let csv = MetricsExporter::export(&summary, ExportFormat::Csv).unwrap();
    let structured = MetricsExporter::export(&summary, ExportFormat::Structured).unwrap();

    assert!(!json.is_empty());
    assert!(!csv.is_empty());
    assert!(!structured.is_empty());

    // JSON round-trip
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s2.count, 10);
}

#[test]
fn integration_run_summary_merge_multiple() {
    let summaries: Vec<RunSummary> = (0..5)
        .map(|i| {
            let events: Vec<&str> = if i % 2 == 0 {
                vec!["tool_call", "tool_result"]
            } else {
                vec!["error", "warning"]
            };
            RunSummary::from_events(&events, 100)
        })
        .collect();

    let mut combined = RunSummary::new();
    for s in &summaries {
        combined.merge(s);
    }
    assert_eq!(combined.total_events, 10);
    assert_eq!(combined.total_duration_ms, 500);
    assert_eq!(combined.tool_call_count, 3); // i=0,2,4
    assert_eq!(combined.error_count, 2); // i=1,3
    assert_eq!(combined.warning_count, 2); // i=1,3
}

#[test]
fn integration_histogram_merge_multiple() {
    let mut combined = LatencyHistogram::new();
    for i in 0..5 {
        let mut h = LatencyHistogram::new();
        h.record((i * 10 + 10) as f64);
        h.record((i * 10 + 15) as f64);
        combined.merge(&h);
    }
    assert_eq!(combined.count(), 10);
    assert!(combined.min().unwrap() >= 10.0);
    assert!(combined.max().unwrap() <= 55.0);
}

#[test]
fn integration_concurrent_run_summaries() {
    let collector = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..10 {
        let cc = collector.clone();
        handles.push(thread::spawn(move || {
            cc.record(simple_run("concurrent", i * 50, if i > 7 { 1 } else { 0 }));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let summary = collector.summary();
    assert_eq!(summary.count, 10);
    assert_eq!(summary.backend_counts["concurrent"], 10);
}

#[test]
fn integration_csv_export_roundtrip_field_count() {
    let runs = vec![simple_run("a", 100, 0), simple_run("b", 200, 1)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    let header_cols = lines[0].split(',').count();
    for line in &lines[1..] {
        assert_eq!(line.split(',').count(), header_cols);
    }
}

#[test]
fn integration_structured_export_parseable() {
    let c = MetricsCollector::new();
    c.record(simple_run("test", 100, 0));
    let summary = c.summary();
    let out = MetricsExporter::export_structured(&summary).unwrap();
    for line in out.lines() {
        assert!(
            line.contains('='),
            "Each line should be key=value: {}",
            line
        );
    }
}

#[test]
fn integration_cost_with_histogram() {
    let mut h = LatencyHistogram::new();
    let mut estimator = CostEstimator::new();
    estimator.set_pricing(
        "model-a",
        ModelPricing {
            input_cost_per_token: 0.0001,
            output_cost_per_token: 0.0002,
        },
    );

    let durations = [50.0, 100.0, 150.0, 200.0, 250.0];
    for d in durations {
        h.record(d);
    }

    let cost = estimator.estimate("model-a", 500, 250).unwrap();
    assert!(cost > 0.0);
    assert_eq!(h.count(), 5);
    assert!((h.p50() - 150.0).abs() < f64::EPSILON);
}

#[test]
fn integration_export_format_serde() {
    let json = serde_json::to_string(&ExportFormat::Json).unwrap();
    let csv = serde_json::to_string(&ExportFormat::Csv).unwrap();
    let structured = serde_json::to_string(&ExportFormat::Structured).unwrap();
    assert_eq!(json, "\"json\"");
    assert_eq!(csv, "\"csv\"");
    assert_eq!(structured, "\"structured\"");

    let rt: ExportFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExportFormat::Json);
}

#[test]
fn integration_end_to_end_multi_backend() {
    // Simulate multi-backend run
    let c = MetricsCollector::new();
    c.record(simple_run("openai", 150, 0));
    c.record(simple_run("anthropic", 200, 1));
    c.record(simple_run("openai", 100, 0));

    let summary = c.summary();
    assert_eq!(summary.backend_counts["openai"], 2);
    assert_eq!(summary.backend_counts["anthropic"], 1);

    // Event-level summary
    let run_sum = RunSummary::from_events(
        &["run_started", "tool_call", "error", "run_completed"],
        summary.mean_duration_ms as u64,
    );
    assert_eq!(run_sum.total_events, 4);
    assert!(run_sum.has_errors());

    // Cost estimation
    let mut estimator = CostEstimator::new();
    estimator.set_pricing(
        "openai",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    estimator.set_pricing(
        "anthropic",
        ModelPricing {
            input_cost_per_token: 0.00001,
            output_cost_per_token: 0.00003,
        },
    );

    let runs = c.runs();
    let usages: Vec<(&str, u64, u64)> = runs
        .iter()
        .map(|r| (r.backend_name.as_str(), r.tokens_in, r.tokens_out))
        .collect();
    let total_cost = estimator.estimate_total(&usages);
    assert!(total_cost > 0.0);

    // Export
    let json = MetricsExporter::export(&summary, ExportFormat::Json).unwrap();
    let csv = MetricsExporter::export_csv(&c.runs()).unwrap();
    assert!(!json.is_empty());
    assert!(csv.lines().count() == 4); // header + 3 runs
}

#[test]
fn integration_histogram_buckets_with_collector() {
    let c = MetricsCollector::new();
    let mut h = LatencyHistogram::new();

    for d in [10, 50, 100, 500, 1000, 5000] {
        c.record(simple_run("test", d, 0));
        h.record(d as f64);
    }

    let buckets = h.buckets(&[50.0, 200.0, 1000.0]);
    // [0,50): 10 → 1
    // [50,200): 50, 100 → 2
    // [200,1000): 500 → 1
    // [1000,∞): 1000, 5000 → 2
    assert_eq!(buckets, vec![1, 2, 1, 2]);

    let summary = c.summary();
    assert_eq!(summary.count, 6);
}
