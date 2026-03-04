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
//! Comprehensive tests for the abp-telemetry crate.

use abp_telemetry::{
    JsonExporter, MetricsCollector, MetricsSummary, RunMetrics, TelemetryExporter, TelemetrySpan,
};
use std::sync::Arc;
use std::thread;

// =========================================================================
// Helpers
// =========================================================================

fn make_metrics(backend: &str, duration: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test_dialect".to_string(),
        duration_ms: duration,
        events_count: 10,
        tokens_in: 50,
        tokens_out: 100,
        tool_calls_count: 2,
        errors_count: errors,
        emulations_applied: 1,
    }
}

fn make_metrics_tokens(backend: &str, tok_in: u64, tok_out: u64) -> RunMetrics {
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

// =========================================================================
// 1. RunMetrics construction & defaults
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
fn run_metrics_field_assignment() {
    let m = make_metrics("mock", 42, 1);
    assert_eq!(m.backend_name, "mock");
    assert_eq!(m.dialect, "test_dialect");
    assert_eq!(m.duration_ms, 42);
    assert_eq!(m.errors_count, 1);
}

#[test]
fn run_metrics_clone_equals_original() {
    let m = make_metrics("a", 100, 0);
    let m2 = m.clone();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_partial_eq_different() {
    let a = make_metrics("a", 100, 0);
    let b = make_metrics("b", 100, 0);
    assert_ne!(a, b);
}

#[test]
fn run_metrics_partial_eq_duration_differs() {
    let a = make_metrics("x", 100, 0);
    let b = make_metrics("x", 200, 0);
    assert_ne!(a, b);
}

#[test]
fn run_metrics_debug_impl() {
    let m = RunMetrics::default();
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("RunMetrics"));
}

#[test]
fn run_metrics_all_fields_populated() {
    let m = RunMetrics {
        backend_name: "back".into(),
        dialect: "dial".into(),
        duration_ms: 1,
        events_count: 2,
        tokens_in: 3,
        tokens_out: 4,
        tool_calls_count: 5,
        errors_count: 6,
        emulations_applied: 7,
    };
    assert_eq!(m.backend_name, "back");
    assert_eq!(m.dialect, "dial");
    assert_eq!(m.duration_ms, 1);
    assert_eq!(m.events_count, 2);
    assert_eq!(m.tokens_in, 3);
    assert_eq!(m.tokens_out, 4);
    assert_eq!(m.tool_calls_count, 5);
    assert_eq!(m.errors_count, 6);
    assert_eq!(m.emulations_applied, 7);
}

// =========================================================================
// 2. RunMetrics serde roundtrip
// =========================================================================

#[test]
fn run_metrics_serde_roundtrip_default() {
    let m = RunMetrics::default();
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_serde_roundtrip_populated() {
    let m = make_metrics("serde_test", 999, 3);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_json_has_expected_keys() {
    let m = make_metrics("k", 1, 0);
    let v: serde_json::Value = serde_json::to_value(&m).unwrap();
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

#[test]
fn run_metrics_deserialize_from_manual_json() {
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
fn run_metrics_serde_pretty_roundtrip() {
    let m = make_metrics("pretty", 123, 0);
    let json = serde_json::to_string_pretty(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn run_metrics_serde_large_values() {
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

// =========================================================================
// 3. MetricsSummary defaults & serde
// =========================================================================

#[test]
fn metrics_summary_default_all_zero() {
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
    let s = MetricsSummary::default();
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_debug_impl() {
    let s = MetricsSummary::default();
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("MetricsSummary"));
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 1));
    c.record(make_metrics("b", 200, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_serde_default_roundtrip() {
    let s = MetricsSummary::default();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_json_keys() {
    let s = MetricsSummary::default();
    let v: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert!(v.get("count").is_some());
    assert!(v.get("mean_duration_ms").is_some());
    assert!(v.get("p50_duration_ms").is_some());
    assert!(v.get("p99_duration_ms").is_some());
    assert!(v.get("total_tokens_in").is_some());
    assert!(v.get("total_tokens_out").is_some());
    assert!(v.get("error_rate").is_some());
    assert!(v.get("backend_counts").is_some());
}

// =========================================================================
// 4. MetricsCollector basic operations
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
    c.record(make_metrics("a", 10, 0));
    assert_eq!(c.len(), 1);
    c.record(make_metrics("b", 20, 0));
    assert_eq!(c.len(), 2);
}

#[test]
fn collector_not_empty_after_record() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 1, 0));
    assert!(!c.is_empty());
}

#[test]
fn collector_runs_returns_all_in_order() {
    let c = MetricsCollector::new();
    c.record(make_metrics("first", 1, 0));
    c.record(make_metrics("second", 2, 0));
    c.record(make_metrics("third", 3, 0));
    let runs = c.runs();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].backend_name, "first");
    assert_eq!(runs[1].backend_name, "second");
    assert_eq!(runs[2].backend_name, "third");
}

#[test]
fn collector_runs_returns_clone() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 1, 0));
    let runs1 = c.runs();
    let runs2 = c.runs();
    assert_eq!(runs1, runs2);
}

#[test]
fn collector_clear_empties() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 0));
    c.record(make_metrics("b", 2, 0));
    assert_eq!(c.len(), 2);
    c.clear();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn collector_clear_then_record() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 0));
    c.clear();
    c.record(make_metrics("b", 2, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

#[test]
fn collector_clone_shares_state() {
    let c = MetricsCollector::new();
    let c2 = c.clone();
    c.record(make_metrics("shared", 1, 0));
    assert_eq!(c2.len(), 1);
}

#[test]
fn collector_debug_impl() {
    let c = MetricsCollector::new();
    let dbg = format!("{:?}", c);
    assert!(dbg.contains("MetricsCollector"));
}

// =========================================================================
// 5. MetricsCollector::summary() – single run
// =========================================================================

#[test]
fn summary_single_run_count() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 50, 0));
    assert_eq!(c.summary().count, 1);
}

#[test]
fn summary_single_run_mean_equals_duration() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 42, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_single_run_p50_equals_duration() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 42, 0));
    assert!((c.summary().p50_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_single_run_p99_equals_duration() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 42, 0));
    assert!((c.summary().p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn summary_single_run_tokens() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 1, 0));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 50);
    assert_eq!(s.total_tokens_out, 100);
}

#[test]
fn summary_single_run_zero_errors() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 1, 0));
    assert!((c.summary().error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_single_run_with_errors() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 1, 5));
    assert!((c.summary().error_rate - 5.0).abs() < f64::EPSILON);
}

#[test]
fn summary_single_run_backend_count() {
    let c = MetricsCollector::new();
    c.record(make_metrics("mybackend", 1, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["mybackend"], 1);
}

// =========================================================================
// 6. MetricsCollector::summary() – aggregation math
// =========================================================================

#[test]
fn summary_mean_two_runs() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 0));
    c.record(make_metrics("a", 200, 0));
    assert!((c.summary().mean_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_mean_three_runs() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 10, 0));
    c.record(make_metrics("a", 20, 0));
    c.record(make_metrics("a", 30, 0));
    assert!((c.summary().mean_duration_ms - 20.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_odd_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(make_metrics("a", d, 0));
    }
    assert!((c.summary().p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_even_count() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(make_metrics("a", d, 0));
    }
    assert!((c.summary().p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p50_two_runs() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 0));
    c.record(make_metrics("a", 200, 0));
    assert!((c.summary().p50_duration_ms - 150.0).abs() < f64::EPSILON);
}

#[test]
fn summary_p99_100_runs() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(make_metrics("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn summary_p99_two_runs() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 10, 0));
    c.record(make_metrics("a", 1000, 0));
    let s = c.summary();
    // p99 of [10, 1000] with 2 elements: rank = 0.99 * 1 = 0.99
    // lower=0, upper=1, frac=0.99 → 10*0.01 + 1000*0.99 = 0.1 + 990 = 990.1
    assert!((s.p99_duration_ms - 990.1).abs() < 0.01);
}

#[test]
fn summary_total_tokens_in_aggregated() {
    let c = MetricsCollector::new();
    c.record(make_metrics_tokens("a", 100, 0));
    c.record(make_metrics_tokens("b", 250, 0));
    assert_eq!(c.summary().total_tokens_in, 350);
}

#[test]
fn summary_total_tokens_out_aggregated() {
    let c = MetricsCollector::new();
    c.record(make_metrics_tokens("a", 0, 300));
    c.record(make_metrics_tokens("b", 0, 700));
    assert_eq!(c.summary().total_tokens_out, 1000);
}

#[test]
fn summary_error_rate_mixed() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 0));
    c.record(make_metrics("a", 2, 1));
    c.record(make_metrics("a", 3, 0));
    c.record(make_metrics("a", 4, 1));
    // 2 errors / 4 runs = 0.5
    assert!((c.summary().error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_all_errors() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 1));
    c.record(make_metrics("a", 2, 1));
    assert!((c.summary().error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_no_errors() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 0));
    c.record(make_metrics("a", 2, 0));
    assert!((c.summary().error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_error_rate_multiple_errors_per_run() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1, 3));
    c.record(make_metrics("a", 2, 0));
    // 3 errors / 2 runs = 1.5
    assert!((c.summary().error_rate - 1.5).abs() < f64::EPSILON);
}

#[test]
fn summary_backend_counts_multiple() {
    let c = MetricsCollector::new();
    c.record(make_metrics("alpha", 1, 0));
    c.record(make_metrics("beta", 2, 0));
    c.record(make_metrics("alpha", 3, 0));
    c.record(make_metrics("gamma", 4, 0));
    c.record(make_metrics("beta", 5, 0));
    c.record(make_metrics("beta", 6, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 3);
    assert_eq!(s.backend_counts["gamma"], 1);
}

#[test]
fn summary_backend_counts_deterministic_order() {
    let c = MetricsCollector::new();
    c.record(make_metrics("zebra", 1, 0));
    c.record(make_metrics("apple", 2, 0));
    c.record(make_metrics("mango", 3, 0));
    let s = c.summary();
    let keys: Vec<&String> = s.backend_counts.keys().collect();
    assert_eq!(keys, vec!["apple", "mango", "zebra"]);
}

// =========================================================================
// 7. MetricsCollector::summary() – empty collector
// =========================================================================

#[test]
fn empty_collector_summary_is_default() {
    let c = MetricsCollector::new();
    assert_eq!(c.summary(), MetricsSummary::default());
}

#[test]
fn cleared_collector_summary_is_default() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 1));
    c.clear();
    assert_eq!(c.summary(), MetricsSummary::default());
}

// =========================================================================
// 8. Duration / timing scenarios
// =========================================================================

#[test]
fn summary_identical_durations() {
    let c = MetricsCollector::new();
    for _ in 0..5 {
        c.record(make_metrics("a", 100, 0));
    }
    let s = c.summary();
    assert!((s.mean_duration_ms - 100.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 100.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 100.0).abs() < f64::EPSILON);
}

#[test]
fn summary_zero_durations() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 0, 0));
    c.record(make_metrics("a", 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_large_duration_values() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 1_000_000_000, 0));
    c.record(make_metrics("a", 2_000_000_000, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 1_500_000_000.0).abs() < 1.0);
}

#[test]
fn summary_single_outlier_high() {
    let c = MetricsCollector::new();
    for _ in 0..99 {
        c.record(make_metrics("a", 10, 0));
    }
    c.record(make_metrics("a", 10000, 0));
    let s = c.summary();
    // mean should be pulled up
    assert!(s.mean_duration_ms > 10.0);
    // p50 should stay near 10
    assert!(s.p50_duration_ms <= 10.0 + f64::EPSILON);
    // p99 should be high
    assert!(s.p99_duration_ms > 10.0);
}

#[test]
fn summary_descending_durations_same_as_ascending() {
    let c1 = MetricsCollector::new();
    let c2 = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c1.record(make_metrics("a", d, 0));
    }
    for d in [50, 40, 30, 20, 10] {
        c2.record(make_metrics("a", d, 0));
    }
    let s1 = c1.summary();
    let s2 = c2.summary();
    assert!((s1.mean_duration_ms - s2.mean_duration_ms).abs() < f64::EPSILON);
    assert!((s1.p50_duration_ms - s2.p50_duration_ms).abs() < f64::EPSILON);
    assert!((s1.p99_duration_ms - s2.p99_duration_ms).abs() < f64::EPSILON);
}

// =========================================================================
// 9. TelemetrySpan construction
// =========================================================================

#[test]
fn span_new_has_name() {
    let span = TelemetrySpan::new("my_op");
    assert_eq!(span.name, "my_op");
}

#[test]
fn span_new_empty_attributes() {
    let span = TelemetrySpan::new("op");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_with_attribute_adds_entry() {
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
        .with_attribute("key", "old")
        .with_attribute("key", "new");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "new");
}

#[test]
fn span_attributes_sorted_btree() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z", "1")
        .with_attribute("a", "2")
        .with_attribute("m", "3");
    let keys: Vec<&String> = span.attributes.keys().collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn span_name_from_string() {
    let name = String::from("dynamic_name");
    let span = TelemetrySpan::new(name);
    assert_eq!(span.name, "dynamic_name");
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
    let span = TelemetrySpan::new("debug_op");
    let dbg = format!("{:?}", span);
    assert!(dbg.contains("TelemetrySpan"));
    assert!(dbg.contains("debug_op"));
}

#[test]
fn span_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_empty_attribute_key_value() {
    let span = TelemetrySpan::new("op").with_attribute("", "");
    assert_eq!(span.attributes[""], "");
}

#[test]
fn span_unicode_name_and_attributes() {
    let span = TelemetrySpan::new("日本語")
        .with_attribute("键", "值")
        .with_attribute("emoji", "🚀");
    assert_eq!(span.name, "日本語");
    assert_eq!(span.attributes["键"], "值");
    assert_eq!(span.attributes["emoji"], "🚀");
}

// =========================================================================
// 10. TelemetrySpan serde
// =========================================================================

#[test]
fn span_serde_roundtrip_no_attributes() {
    let span = TelemetrySpan::new("bare");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "bare");
    assert!(span2.attributes.is_empty());
}

#[test]
fn span_serde_roundtrip_with_attributes() {
    let span = TelemetrySpan::new("rich")
        .with_attribute("backend", "mock")
        .with_attribute("run_id", "abc-123");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "rich");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["run_id"], "abc-123");
}

#[test]
fn span_deserialize_from_manual_json() {
    let json = r#"{"name":"manual","attributes":{"x":"y"}}"#;
    let span: TelemetrySpan = serde_json::from_str(json).unwrap();
    assert_eq!(span.name, "manual");
    assert_eq!(span.attributes["x"], "y");
}

#[test]
fn span_json_attribute_order_deterministic() {
    let span = TelemetrySpan::new("op")
        .with_attribute("z", "1")
        .with_attribute("a", "2");
    let json = serde_json::to_string(&span).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should serialize keys in order");
}

// =========================================================================
// 11. TelemetrySpan::emit() – just ensure no panic
// =========================================================================

#[test]
fn span_emit_does_not_panic() {
    let span = TelemetrySpan::new("emit_test").with_attribute("safe", "true");
    span.emit(); // should not panic even without a subscriber
}

#[test]
fn span_emit_empty_does_not_panic() {
    let span = TelemetrySpan::new("");
    span.emit();
}

// =========================================================================
// 12. JsonExporter
// =========================================================================

#[test]
fn json_exporter_default() {
    let _e = JsonExporter;
}

#[test]
fn json_exporter_debug_impl() {
    let e = JsonExporter;
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("JsonExporter"));
}

#[test]
fn json_exporter_empty_summary() {
    let e = JsonExporter;
    let s = MetricsSummary::default();
    let json = e.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 0);
}

#[test]
fn json_exporter_single_run() {
    let c = MetricsCollector::new();
    c.record(make_metrics("mock", 100, 0));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 1);
    assert_eq!(v["mean_duration_ms"], 100.0);
}

#[test]
fn json_exporter_multiple_backends() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 10, 0));
    c.record(make_metrics("b", 20, 0));
    c.record(make_metrics("a", 30, 0));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["backend_counts"]["a"], 2);
    assert_eq!(v["backend_counts"]["b"], 1);
}

#[test]
fn json_exporter_output_is_pretty_printed() {
    let s = MetricsSummary::default();
    let json = JsonExporter.export(&s).unwrap();
    assert!(
        json.contains('\n'),
        "pretty-printed JSON should contain newlines"
    );
}

#[test]
fn json_exporter_returns_valid_json() {
    let c = MetricsCollector::new();
    for i in 0..10 {
        c.record(make_metrics(&format!("b{i}"), i * 10, i % 3));
    }
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
}

#[test]
fn json_exporter_backend_counts_sorted_in_output() {
    let c = MetricsCollector::new();
    c.record(make_metrics("z_backend", 10, 0));
    c.record(make_metrics("a_backend", 20, 0));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let a_pos = json.find("a_backend").unwrap();
    let z_pos = json.find("z_backend").unwrap();
    assert!(a_pos < z_pos);
}

// =========================================================================
// 13. TelemetryExporter trait object usage
// =========================================================================

#[test]
fn exporter_as_trait_object() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let result = exporter.export(&s);
    assert!(result.is_ok());
}

#[test]
fn exporter_arc_trait_object() {
    let exporter: Arc<dyn TelemetryExporter> = Arc::new(JsonExporter);
    let s = MetricsSummary::default();
    let json = exporter.export(&s).unwrap();
    assert!(!json.is_empty());
}

// =========================================================================
// 14. Concurrent metric recording (thread safety)
// =========================================================================

#[test]
fn concurrent_10_threads_recording() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(make_metrics("thread", i * 10, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 10);
}

#[test]
fn concurrent_100_threads_recording() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..100)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(make_metrics("t", i, 0));
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
    c.record(make_metrics("pre", 10, 0));
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(make_metrics("t", 20, 0));
                let _ = cc.summary();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 6);
}

#[test]
fn concurrent_clear_and_record() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(make_metrics("init", 1, 0));
    }
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                if i == 0 {
                    cc.clear();
                } else {
                    cc.record(make_metrics("after", 2, 0));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // We can't predict exact count due to race, but it shouldn't panic
    let _ = c.len();
    let _ = c.summary();
}

#[test]
fn concurrent_runs_access() {
    let c = MetricsCollector::new();
    for i in 0..20 {
        c.record(make_metrics("x", i, 0));
    }
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                let runs = cc.runs();
                assert_eq!(runs.len(), 20);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

// =========================================================================
// 15. Edge cases – overflow & extreme values
// =========================================================================

#[test]
fn run_metrics_max_u64_values() {
    let m = RunMetrics {
        backend_name: "max".into(),
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
fn summary_with_very_large_token_counts() {
    let c = MetricsCollector::new();
    c.record(make_metrics_tokens("a", u64::MAX / 2, u64::MAX / 2));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, u64::MAX / 2);
    assert_eq!(s.total_tokens_out, u64::MAX / 2);
}

#[test]
fn summary_single_zero_duration_run() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!((s.p50_duration_ms - 0.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 0.0).abs() < f64::EPSILON);
}

#[test]
fn many_backends_in_summary() {
    let c = MetricsCollector::new();
    for i in 0..50 {
        c.record(make_metrics(&format!("backend_{i:03}"), i, 0));
    }
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 50);
    assert_eq!(s.count, 50);
}

#[test]
fn summary_after_1000_runs() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(make_metrics("bulk", i, 0));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    assert!((s.mean_duration_ms - 499.5).abs() < 0.01);
}

// =========================================================================
// 16. Empty string / special character edge cases
// =========================================================================

#[test]
fn run_metrics_empty_backend_name() {
    let m = RunMetrics {
        backend_name: String::new(),
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.backend_name, "");
}

#[test]
fn run_metrics_special_chars_in_name() {
    let m = RunMetrics {
        backend_name: "back/end:special!@#$%".into(),
        dialect: "dial\"ect".into(),
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn span_special_chars_in_attributes() {
    let span = TelemetrySpan::new("op")
        .with_attribute("path", "/usr/local/bin")
        .with_attribute("query", "SELECT * FROM t WHERE x = 'y'");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.attributes["path"], "/usr/local/bin");
}

#[test]
fn run_metrics_very_long_backend_name() {
    let long_name = "x".repeat(10000);
    let m = RunMetrics {
        backend_name: long_name.clone(),
        ..RunMetrics::default()
    };
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m2.backend_name, long_name);
}

// =========================================================================
// 17. Multiple summary calls (idempotency)
// =========================================================================

#[test]
fn summary_called_twice_same_result() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 1));
    c.record(make_metrics("b", 200, 0));
    let s1 = c.summary();
    let s2 = c.summary();
    assert_eq!(s1, s2);
}

#[test]
fn summary_after_additional_record() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 0));
    let s1 = c.summary();
    c.record(make_metrics("b", 200, 0));
    let s2 = c.summary();
    assert_ne!(s1.count, s2.count);
    assert_eq!(s2.count, 2);
}

// =========================================================================
// 18. Collector as shared state (Arc pattern)
// =========================================================================

#[test]
fn collector_in_arc_across_threads() {
    let c = Arc::new(MetricsCollector::new());
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let cc = Arc::clone(&c);
            thread::spawn(move || {
                cc.record(make_metrics("arc", i * 10, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 5);
}

// =========================================================================
// 19. Custom TelemetryExporter impl
// =========================================================================

struct CsvExporter;

impl TelemetryExporter for CsvExporter {
    fn export(&self, summary: &MetricsSummary) -> Result<String, String> {
        Ok(format!(
            "count,mean_duration_ms,error_rate\n{},{},{}",
            summary.count, summary.mean_duration_ms, summary.error_rate
        ))
    }
}

#[test]
fn custom_exporter_csv() {
    let c = MetricsCollector::new();
    c.record(make_metrics("x", 100, 1));
    let s = c.summary();
    let csv = CsvExporter.export(&s).unwrap();
    assert!(csv.starts_with("count,mean_duration_ms,error_rate\n"));
    assert!(csv.contains("1,100,1"));
}

struct FailingExporter;

impl TelemetryExporter for FailingExporter {
    fn export(&self, _summary: &MetricsSummary) -> Result<String, String> {
        Err("export failed".into())
    }
}

#[test]
fn failing_exporter_returns_err() {
    let s = MetricsSummary::default();
    let result = FailingExporter.export(&s);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "export failed");
}

// =========================================================================
// 20. Percentile behavior (tested indirectly via summary)
// =========================================================================

#[test]
fn p50_of_three_values() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 10, 0));
    c.record(make_metrics("a", 20, 0));
    c.record(make_metrics("a", 30, 0));
    // sorted: [10, 20, 30], p50 rank = 0.5 * 2 = 1.0 → value at index 1 = 20
    assert!((c.summary().p50_duration_ms - 20.0).abs() < f64::EPSILON);
}

#[test]
fn p99_of_three_values() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 10, 0));
    c.record(make_metrics("a", 20, 0));
    c.record(make_metrics("a", 30, 0));
    // sorted: [10, 20, 30], p99 rank = 0.99 * 2 = 1.98
    // lower=1 (20), upper=2 (30), frac=0.98 → 20*0.02 + 30*0.98 = 0.4 + 29.4 = 29.8
    assert!((c.summary().p99_duration_ms - 29.8).abs() < 0.01);
}

#[test]
fn percentile_all_same_values() {
    let c = MetricsCollector::new();
    for _ in 0..10 {
        c.record(make_metrics("a", 42, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 42.0).abs() < f64::EPSILON);
    assert!((s.p99_duration_ms - 42.0).abs() < f64::EPSILON);
}

#[test]
fn percentile_two_distinct_values() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 0, 0));
    c.record(make_metrics("a", 100, 0));
    // p50: rank = 0.5 * 1 = 0.5 → 0 * 0.5 + 100 * 0.5 = 50
    assert!((c.summary().p50_duration_ms - 50.0).abs() < f64::EPSILON);
}

// =========================================================================
// 21. MetricsSummary partial equality checks
// =========================================================================

#[test]
fn metrics_summary_partial_eq_identical() {
    let a = MetricsSummary::default();
    let b = MetricsSummary::default();
    assert_eq!(a, b);
}

#[test]
fn metrics_summary_partial_eq_different() {
    let a = MetricsSummary::default();
    let b = MetricsSummary {
        count: 1,
        ..Default::default()
    };
    assert_ne!(a, b);
}

// =========================================================================
// 22. Backend counts with same name
// =========================================================================

#[test]
fn backend_counts_accumulate() {
    let c = MetricsCollector::new();
    for _ in 0..100 {
        c.record(make_metrics("only_one", 10, 0));
    }
    let s = c.summary();
    assert_eq!(s.backend_counts.len(), 1);
    assert_eq!(s.backend_counts["only_one"], 100);
}

// =========================================================================
// 23. Serde edge cases for MetricsSummary with populated backend_counts
// =========================================================================

#[test]
fn metrics_summary_serde_with_backend_counts() {
    let mut bc = std::collections::BTreeMap::new();
    bc.insert("alpha".into(), 2);
    bc.insert("beta".into(), 1);
    let s = MetricsSummary {
        count: 3,
        mean_duration_ms: 50.0,
        backend_counts: bc,
        ..Default::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn metrics_summary_deserialize_from_manual_json() {
    let json = r#"{
        "count": 2,
        "mean_duration_ms": 42.5,
        "p50_duration_ms": 40.0,
        "p99_duration_ms": 45.0,
        "total_tokens_in": 100,
        "total_tokens_out": 200,
        "error_rate": 0.5,
        "backend_counts": {"x": 1, "y": 1}
    }"#;
    let s: MetricsSummary = serde_json::from_str(json).unwrap();
    assert_eq!(s.count, 2);
    assert_eq!(s.backend_counts["x"], 1);
    assert_eq!(s.backend_counts["y"], 1);
}

// =========================================================================
// 24. JsonExporter output re-parseable as MetricsSummary
// =========================================================================

#[test]
fn json_exporter_output_deserializable_as_metrics_summary() {
    let c = MetricsCollector::new();
    c.record(make_metrics("a", 100, 0));
    c.record(make_metrics("b", 200, 1));
    let s = c.summary();
    let json = JsonExporter.export(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

// =========================================================================
// 25. RunMetrics value-based JSON checks
// =========================================================================

#[test]
fn run_metrics_json_value_types() {
    let m = make_metrics("check", 42, 1);
    let v: serde_json::Value = serde_json::to_value(&m).unwrap();
    assert!(v["backend_name"].is_string());
    assert!(v["dialect"].is_string());
    assert!(v["duration_ms"].is_number());
    assert!(v["events_count"].is_number());
    assert!(v["tokens_in"].is_number());
    assert!(v["tokens_out"].is_number());
    assert!(v["tool_calls_count"].is_number());
    assert!(v["errors_count"].is_number());
    assert!(v["emulations_applied"].is_number());
}

// =========================================================================
// 26. TelemetrySpan with many attributes
// =========================================================================

#[test]
fn span_with_100_attributes() {
    let mut span = TelemetrySpan::new("big_span");
    for i in 0..100 {
        span = span.with_attribute(format!("key_{i:03}"), format!("val_{i}"));
    }
    assert_eq!(span.attributes.len(), 100);
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.attributes.len(), 100);
}

// =========================================================================
// 27. Collector record preserves all fields
// =========================================================================

#[test]
fn collector_preserves_all_fields_of_run_metrics() {
    let c = MetricsCollector::new();
    let m = RunMetrics {
        backend_name: "b".into(),
        dialect: "d".into(),
        duration_ms: 1,
        events_count: 2,
        tokens_in: 3,
        tokens_out: 4,
        tool_calls_count: 5,
        errors_count: 6,
        emulations_applied: 7,
    };
    c.record(m.clone());
    let runs = c.runs();
    assert_eq!(runs[0], m);
}

// =========================================================================
// 28. Summary token aggregation with custom values
// =========================================================================

#[test]
fn summary_tokens_aggregated_across_different_values() {
    let c = MetricsCollector::new();
    c.record(make_metrics_tokens("a", 10, 20));
    c.record(make_metrics_tokens("b", 30, 40));
    c.record(make_metrics_tokens("c", 50, 60));
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 90);
    assert_eq!(s.total_tokens_out, 120);
}
