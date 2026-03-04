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
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::approx_constant)]
//! Deep tests for the telemetry pipeline covering span creation, nesting,
//! metrics collection, event recording, context propagation, exporter
//! integration, filtering, attribute types, span status, timing,
//! concurrency, pipeline construction, and serialization.
#![allow(clippy::float_cmp)]

use abp_telemetry::hooks::*;
use abp_telemetry::metrics::*;
use abp_telemetry::pipeline::*;
use abp_telemetry::spans;
use abp_telemetry::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mk_run(backend: &str, dur: u64, tokens_in: u64, tokens_out: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: "test_dialect".into(),
        duration_ms: dur,
        events_count: 4,
        tokens_in,
        tokens_out,
        tool_calls_count: 2,
        errors_count: errors,
        emulations_applied: 0,
    }
}

fn mk_event(
    event_type: TelemetryEventType,
    run_id: Option<&str>,
    backend: Option<&str>,
    duration_ms: Option<u64>,
) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".into(),
        event_type,
        run_id: run_id.map(String::from),
        backend: backend.map(String::from),
        metadata: BTreeMap::new(),
        duration_ms,
    }
}

fn mk_event_with_meta(
    event_type: TelemetryEventType,
    meta: BTreeMap<String, serde_json::Value>,
) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".into(),
        event_type,
        run_id: None,
        backend: None,
        metadata: meta,
        duration_ms: None,
    }
}

// =========================================================================
// 1. Span creation
// =========================================================================

#[test]
fn span_creation_basic_name() {
    let span = TelemetrySpan::new("test_operation");
    assert_eq!(span.name, "test_operation");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_creation_with_single_attribute() {
    let span = TelemetrySpan::new("op").with_attribute("key", "value");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["key"], "value");
}

#[test]
fn span_creation_with_multiple_attributes() {
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
fn span_creation_attribute_overwrite() {
    let span = TelemetrySpan::new("op")
        .with_attribute("k", "old")
        .with_attribute("k", "new");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["k"], "new");
}

#[test]
fn span_creation_empty_name() {
    let span = TelemetrySpan::new("");
    assert_eq!(span.name, "");
}

#[test]
fn span_creation_unicode_name() {
    let span = TelemetrySpan::new("操作_émoji_🚀");
    assert_eq!(span.name, "操作_émoji_🚀");
}

#[test]
fn span_emit_does_not_panic() {
    let span = TelemetrySpan::new("emit_test").with_attribute("k", "v");
    span.emit(); // should not panic
}

// =========================================================================
// 2. Span nesting — tracing span helpers
// =========================================================================

#[test]
fn request_span_enter_and_drop() {
    let span = spans::request_span("wo-100", "refactor", "mapped");
    let _guard = span.enter();
}

#[test]
fn event_span_nested_inside_request() {
    let parent = spans::request_span("wo-200", "task", "passthrough");
    let _p = parent.enter();
    let child = spans::event_span("tool_call", 1);
    let _c = child.enter();
}

#[test]
fn backend_span_nested_inside_request() {
    let parent = spans::request_span("wo-300", "task", "mapped");
    let _p = parent.enter();
    let child = spans::backend_span("sidecar:node");
    let _c = child.enter();
}

#[test]
fn multiple_sibling_event_spans() {
    let parent = spans::request_span("wo-400", "build", "mapped");
    let _p = parent.enter();
    for i in 0..5 {
        let child = spans::event_span("step", i);
        let _c = child.enter();
    }
}

// =========================================================================
// 3. Metrics collection — counters, gauges, histograms
// =========================================================================

#[test]
fn request_counter_multiple_dimensions() {
    let c = RequestCounter::new();
    c.increment("mock", "openai", "success");
    c.increment("mock", "anthropic", "success");
    c.increment("sidecar", "openai", "error");
    assert_eq!(c.get("mock", "openai", "success"), 1);
    assert_eq!(c.get("mock", "anthropic", "success"), 1);
    assert_eq!(c.get("sidecar", "openai", "error"), 1);
    assert_eq!(c.total(), 3);
}

#[test]
fn error_counter_multiple_codes() {
    let c = ErrorCounter::new();
    c.increment("timeout");
    c.increment("timeout");
    c.increment("rate_limit");
    c.increment("auth_fail");
    assert_eq!(c.get("timeout"), 2);
    assert_eq!(c.get("rate_limit"), 1);
    assert_eq!(c.get("auth_fail"), 1);
    assert_eq!(c.total(), 4);
}

#[test]
fn gauge_goes_negative() {
    let g = ActiveRequestGauge::new();
    g.decrement();
    assert_eq!(g.get(), -1);
}

#[test]
fn gauge_high_watermark_pattern() {
    let g = ActiveRequestGauge::new();
    let mut max_val = 0i64;
    for _ in 0..5 {
        g.increment();
        max_val = max_val.max(g.get());
    }
    for _ in 0..3 {
        g.decrement();
    }
    assert_eq!(max_val, 5);
    assert_eq!(g.get(), 2);
}

#[test]
fn token_accumulator_multiple_adds() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.add(150, 250);
    t.add(50, 50);
    assert_eq!(t.total_input(), 300);
    assert_eq!(t.total_output(), 500);
    assert_eq!(t.total(), 800);
}

#[test]
fn histogram_basic_statistics() {
    let mut h = LatencyHistogram::new();
    for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
        h.record(v);
    }
    assert_eq!(h.count(), 5);
    assert!(!h.is_empty());
    assert_eq!(h.min(), Some(10.0));
    assert_eq!(h.max(), Some(50.0));
    assert!((h.mean() - 30.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_percentiles() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    assert!((h.p50() - 50.5).abs() < 1.0);
    assert!(h.p95() > 94.0);
    assert!(h.p99() > 98.0);
}

#[test]
fn histogram_buckets() {
    let mut h = LatencyHistogram::new();
    for v in [5.0, 15.0, 25.0, 35.0, 150.0] {
        h.record(v);
    }
    let counts = h.buckets(&[10.0, 20.0, 50.0]);
    // [0,10) = 1, [10,20) = 1, [20,50) = 2, [50,∞) = 1
    assert_eq!(counts, vec![1, 1, 2, 1]);
}

#[test]
fn histogram_empty_stats() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.min(), None);
    assert_eq!(h.max(), None);
    assert_eq!(h.mean(), 0.0);
    assert_eq!(h.p50(), 0.0);
}

#[test]
fn histogram_single_value() {
    let mut h = LatencyHistogram::new();
    h.record(42.5);
    assert_eq!(h.min(), Some(42.5));
    assert_eq!(h.max(), Some(42.5));
    assert_eq!(h.p50(), 42.5);
    assert_eq!(h.p99(), 42.5);
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
    assert_eq!(h1.max(), Some(30.0));
}

// =========================================================================
// 4. Event recording — RunSummary
// =========================================================================

#[test]
fn run_summary_from_events() {
    let s = RunSummary::from_events(&["tool_call", "error", "tool_call", "warning"], 500);
    assert_eq!(s.total_events, 4);
    assert_eq!(s.tool_call_count, 2);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.total_duration_ms, 500);
}

#[test]
fn run_summary_record_unknown_event() {
    let mut s = RunSummary::new();
    s.record_event("custom_event");
    assert_eq!(s.total_events, 1);
    assert_eq!(s.event_counts["custom_event"], 1);
    assert_eq!(s.error_count, 0);
    assert_eq!(s.warning_count, 0);
}

#[test]
fn run_summary_has_errors() {
    let mut s = RunSummary::new();
    assert!(!s.has_errors());
    s.record_event("error");
    assert!(s.has_errors());
}

#[test]
fn run_summary_error_rate() {
    let s = RunSummary::from_events(&["error", "tool_call", "tool_call", "error"], 100);
    assert!((s.error_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn run_summary_error_rate_no_events() {
    let s = RunSummary::new();
    assert_eq!(s.error_rate(), 0.0);
}

#[test]
fn run_summary_merge() {
    let mut a = RunSummary::from_events(&["tool_call", "error"], 100);
    let b = RunSummary::from_events(&["warning", "tool_call"], 200);
    a.merge(&b);
    assert_eq!(a.total_events, 4);
    assert_eq!(a.tool_call_count, 2);
    assert_eq!(a.error_count, 1);
    assert_eq!(a.warning_count, 1);
    assert_eq!(a.total_duration_ms, 300);
}

#[test]
fn run_summary_set_duration() {
    let mut s = RunSummary::new();
    s.set_duration(999);
    assert_eq!(s.total_duration_ms, 999);
}

// =========================================================================
// 5. Context propagation — TelemetryCollector run filtering
// =========================================================================

#[test]
fn collector_run_events_filters_by_run_id() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("run-1"),
        None,
        None,
    ));
    c.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("run-2"),
        None,
        None,
    ));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        Some("run-1"),
        None,
        Some(100),
    ));
    let run1_events = c.run_events("run-1");
    assert_eq!(run1_events.len(), 2);
    let run2_events = c.run_events("run-2");
    assert_eq!(run2_events.len(), 1);
}

#[test]
fn collector_run_events_missing_run_id() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    let events = c.run_events("any");
    assert!(events.is_empty());
}

#[test]
fn collector_events_of_type() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(mk_event(TelemetryEventType::RunFailed, None, None, None));
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
    let failed = c.events_of_type(TelemetryEventType::RunFailed);
    assert_eq!(failed.len(), 1);
}

// =========================================================================
// 6. Exporter integration — export to in-memory collector
// =========================================================================

#[test]
fn json_exporter_roundtrip() {
    let c = MetricsCollector::new();
    c.record(mk_run("mock", 100, 50, 75, 0));
    c.record(mk_run("sidecar", 200, 80, 120, 1));
    let summary = c.summary();
    let exporter = JsonExporter;
    let json = exporter.export(&summary).unwrap();
    let parsed: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.count, 2);
    assert_eq!(parsed.total_tokens_in, 130);
    assert_eq!(parsed.total_tokens_out, 195);
}

#[test]
fn metrics_exporter_json_format() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 50, 10, 20, 0));
    let s = c.summary();
    let out = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["count"], 1);
}

#[test]
fn metrics_exporter_csv_format() {
    let c = MetricsCollector::new();
    c.record(mk_run("csv_test", 100, 10, 20, 0));
    let s = c.summary();
    let out = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(out.starts_with("count,"));
    assert!(out.contains("100.00"));
}

#[test]
fn metrics_exporter_structured_format() {
    let c = MetricsCollector::new();
    c.record(mk_run("struct_test", 42, 10, 20, 0));
    let s = c.summary();
    let out = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(out.contains("count=1"));
    assert!(out.contains("mean_duration_ms=42.00"));
}

#[test]
fn metrics_exporter_csv_runs() {
    let runs = vec![mk_run("a", 10, 1, 2, 0), mk_run("b", 20, 3, 4, 1)];
    let out = MetricsExporter::export_csv(&runs).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3); // header + 2 rows
    assert!(lines[0].starts_with("backend_name,"));
    assert!(lines[1].starts_with("a,"));
    assert!(lines[2].starts_with("b,"));
}

// =========================================================================
// 7. Filtering — TelemetryFilter
// =========================================================================

#[test]
fn filter_allowed_types_pass() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let e = mk_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_allowed_types_reject() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    let e = mk_event(TelemetryEventType::RunFailed, None, None, None);
    assert!(!f.matches(&e));
}

#[test]
fn filter_min_duration_pass() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(50),
    };
    let e = mk_event(TelemetryEventType::RunCompleted, None, None, Some(100));
    assert!(f.matches(&e));
}

#[test]
fn filter_min_duration_reject() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(50),
    };
    let e = mk_event(TelemetryEventType::RunCompleted, None, None, Some(10));
    assert!(!f.matches(&e));
}

#[test]
fn filter_min_duration_none_passes() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(50),
    };
    let e = mk_event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_combined_both_pass() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(10),
    };
    let e = mk_event(TelemetryEventType::RunCompleted, None, None, Some(50));
    assert!(f.matches(&e));
}

#[test]
fn filter_combined_type_fails() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(10),
    };
    let e = mk_event(TelemetryEventType::RunFailed, None, None, Some(50));
    assert!(!f.matches(&e));
}

#[test]
fn filter_combined_duration_fails() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(100),
    };
    let e = mk_event(TelemetryEventType::RunCompleted, None, None, Some(50));
    assert!(!f.matches(&e));
}

#[test]
fn filter_default_passes_everything() {
    let f = TelemetryFilter::default();
    let events = [
        mk_event(TelemetryEventType::RunStarted, None, None, None),
        mk_event(TelemetryEventType::RunFailed, None, None, Some(0)),
        mk_event(
            TelemetryEventType::BackendSelected,
            None,
            Some("mock"),
            None,
        ),
    ];
    for e in &events {
        assert!(f.matches(e));
    }
}

#[test]
fn collector_with_filter_drops_events() {
    let filter = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(filter);
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].event_type, TelemetryEventType::RunCompleted);
}

// =========================================================================
// 8. Attribute types — string, int, float, bool stored as strings
// =========================================================================

#[test]
fn attribute_string_value() {
    let span = TelemetrySpan::new("op").with_attribute("name", "hello");
    assert_eq!(span.attributes["name"], "hello");
}

#[test]
fn attribute_int_as_string() {
    let span = TelemetrySpan::new("op").with_attribute("count", "42");
    assert_eq!(span.attributes["count"], "42");
}

#[test]
fn attribute_float_as_string() {
    let span = TelemetrySpan::new("op").with_attribute("ratio", "3.14");
    assert_eq!(span.attributes["ratio"], "3.14");
}

#[test]
fn attribute_bool_as_string() {
    let span = TelemetrySpan::new("op").with_attribute("enabled", "true");
    assert_eq!(span.attributes["enabled"], "true");
}

#[test]
fn attribute_empty_value() {
    let span = TelemetrySpan::new("op").with_attribute("empty", "");
    assert_eq!(span.attributes["empty"], "");
}

#[test]
fn attribute_unicode_key_and_value() {
    let span = TelemetrySpan::new("op").with_attribute("clave_名前", "값_🎉");
    assert_eq!(span.attributes["clave_名前"], "값_🎉");
}

#[test]
fn telemetry_event_metadata_types() {
    let mut meta = BTreeMap::new();
    meta.insert("str_key".into(), serde_json::Value::String("hello".into()));
    meta.insert("int_key".into(), serde_json::json!(42));
    meta.insert("float_key".into(), serde_json::json!(3.14));
    meta.insert("bool_key".into(), serde_json::json!(true));
    meta.insert("null_key".into(), serde_json::Value::Null);
    let e = mk_event_with_meta(TelemetryEventType::RunStarted, meta);
    assert_eq!(e.metadata.len(), 5);
    assert_eq!(e.metadata["str_key"], "hello");
    assert_eq!(e.metadata["int_key"], 42);
    assert!(e.metadata["bool_key"].as_bool().unwrap());
    assert!(e.metadata["null_key"].is_null());
}

// =========================================================================
// 9. Span status — modelled via RequestOutcome and hook functions
// =========================================================================

#[test]
fn request_outcome_success() {
    let outcome = RequestOutcome::Success;
    assert_eq!(outcome, RequestOutcome::Success);
}

#[test]
fn request_outcome_error_fields() {
    let outcome = RequestOutcome::Error {
        code: "rate_limit".into(),
        message: "too many requests".into(),
    };
    if let RequestOutcome::Error { code, message } = &outcome {
        assert_eq!(code, "rate_limit");
        assert_eq!(message, "too many requests");
    } else {
        panic!("expected Error variant");
    }
}

#[test]
fn error_classification_variants() {
    assert_eq!(ErrorClassification::Transient.to_string(), "transient");
    assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
    assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
}

#[test]
fn on_error_all_classifications() {
    for cls in [
        ErrorClassification::Transient,
        ErrorClassification::Permanent,
        ErrorClassification::Unknown,
    ] {
        on_error("wo-1", "E001", "msg", cls);
    }
}

#[test]
fn on_request_complete_returns_elapsed() {
    let start = Instant::now();
    let ms = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
    assert!(ms < 5000);
}

// =========================================================================
// 10. Timing — duration measurement accuracy
// =========================================================================

#[test]
fn on_request_start_captures_instant() {
    let before = Instant::now();
    let start = on_request_start("wo-t1", "mock");
    let after = Instant::now();
    assert!(start >= before);
    assert!(start <= after);
}

#[test]
fn request_complete_timing_success() {
    let start = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let ms = on_request_complete("wo-t2", "mock", &RequestOutcome::Success, start);
    assert!(ms >= 5); // allow some tolerance
}

#[test]
fn request_complete_timing_error() {
    let start = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline".into(),
    };
    let ms = on_request_complete("wo-t3", "mock", &outcome, start);
    assert!(ms >= 5);
}

#[test]
fn histogram_timing_records() {
    let mut h = LatencyHistogram::new();
    let t0 = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
    h.record(elapsed);
    assert!(h.min().unwrap() >= 1.0);
}

// =========================================================================
// 11. Concurrency — thread-safe telemetry operations
// =========================================================================

#[test]
fn concurrent_metrics_collector_recording() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(mk_run("thread", i * 10, 10, 20, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
}

#[test]
fn concurrent_request_counter() {
    let c = RequestCounter::new();
    let c_arc = Arc::new(c);
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c_arc.clone();
            thread::spawn(move || {
                cc.increment("mock", "openai", "success");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c_arc.total(), 10);
}

#[test]
fn concurrent_error_counter() {
    let c = ErrorCounter::new();
    let c_arc = Arc::new(c);
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c_arc.clone();
            thread::spawn(move || {
                cc.increment("E001");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c_arc.get("E001"), 10);
}

#[test]
fn concurrent_gauge_operations() {
    let g = Arc::new(ActiveRequestGauge::new());
    let handles: Vec<_> = (0..50)
        .map(|i| {
            let gg = g.clone();
            thread::spawn(move || {
                if i % 2 == 0 {
                    gg.increment();
                } else {
                    gg.decrement();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // 25 increments, 25 decrements
    assert_eq!(g.get(), 0);
}

#[test]
fn concurrent_token_accumulator() {
    let t = Arc::new(TokenAccumulator::new());
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let tt = t.clone();
            thread::spawn(move || {
                tt.add(10, 20);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(t.total_input(), 100);
    assert_eq!(t.total_output(), 200);
}

#[test]
fn concurrent_collector_summary_while_recording() {
    let c = MetricsCollector::new();
    c.record(mk_run("pre", 10, 5, 5, 0));
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(mk_run("t", 20, 10, 10, 0));
                let _ = cc.summary();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 11);
}

// =========================================================================
// 12. Pipeline construction — builder patterns, collector lifecycle
// =========================================================================

#[test]
fn metrics_collector_new_default() {
    let a = MetricsCollector::new();
    let b = MetricsCollector::default();
    assert!(a.is_empty());
    assert!(b.is_empty());
}

#[test]
fn collector_record_clear_rerecord() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 10, 1, 1, 0));
    assert_eq!(c.len(), 1);
    c.clear();
    assert!(c.is_empty());
    c.record(mk_run("b", 20, 2, 2, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

#[test]
fn telemetry_collector_new_is_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
}

#[test]
fn telemetry_collector_clear() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    assert_eq!(c.events().len(), 1);
    c.clear();
    assert!(c.events().is_empty());
}

#[test]
fn cost_estimator_set_and_get_pricing() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let p = est.get_pricing("gpt-4").unwrap();
    assert!((p.input_cost_per_token - 0.00003).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_estimate() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let cost = est.estimate("gpt-4", 1000, 500).unwrap();
    // 1000*0.001 + 500*0.002 = 1.0 + 1.0 = 2.0
    assert!((cost - 2.0).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_unknown_model() {
    let est = CostEstimator::new();
    assert!(est.estimate("unknown", 100, 100).is_none());
}

#[test]
fn cost_estimator_estimate_total() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "m1",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    est.set_pricing(
        "m2",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let total = est.estimate_total(&[("m1", 100, 50), ("m2", 200, 100), ("unknown", 99, 99)]);
    // m1: 100*0.01 + 50*0.02 = 1.0 + 1.0 = 2.0
    // m2: 200*0.001 + 100*0.002 = 0.2 + 0.2 = 0.4
    // unknown: skipped
    assert!((total - 2.4).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_models_list() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    est.set_pricing(
        "claude",
        ModelPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    );
    let models = est.models();
    assert_eq!(models.len(), 2);
    // BTreeMap gives sorted order
    assert_eq!(models[0], "claude");
    assert_eq!(models[1], "gpt-4");
}

// =========================================================================
// 13. Serialization — JSON roundtrip
// =========================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = mk_run("serde_b", 123, 44, 55, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 100, 10, 20, 1));
    c.record(mk_run("b", 200, 30, 40, 0));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("run")
        .with_attribute("backend", "mock")
        .with_attribute("lane", "mapped");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes.len(), 2);
    assert_eq!(span2.attributes["backend"], "mock");
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["tool_call", "error", "warning"], 250);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn latency_histogram_serde_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(1.5);
    h.record(2.5);
    h.record(3.5);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
    assert_eq!(h2.count(), 3);
}

#[test]
fn telemetry_event_serde_roundtrip() {
    let mut meta = BTreeMap::new();
    meta.insert("key".into(), serde_json::json!("value"));
    let e = TelemetryEvent {
        timestamp: "2025-06-01T12:00:00Z".into(),
        event_type: TelemetryEventType::BackendSelected,
        run_id: Some("run-42".into()),
        backend: Some("mock".into()),
        metadata: meta,
        duration_ms: Some(150),
    };
    let json = serde_json::to_string(&e).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.event_type, TelemetryEventType::BackendSelected);
    assert_eq!(e2.run_id.as_deref(), Some("run-42"));
    assert_eq!(e2.duration_ms, Some(150));
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
fn model_pricing_serde_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.00003,
        output_cost_per_token: 0.00006,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn telemetry_event_type_all_variants_serialize() {
    let variants = [
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunCompleted,
        TelemetryEventType::RunFailed,
        TelemetryEventType::BackendSelected,
        TelemetryEventType::RetryAttempted,
        TelemetryEventType::FallbackTriggered,
        TelemetryEventType::CapabilityNegotiated,
        TelemetryEventType::MappingPerformed,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let v2: TelemetryEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, v2);
    }
}

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
}

// =========================================================================
// Additional: TelemetrySummary computation
// =========================================================================

#[test]
fn telemetry_summary_empty_collector() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert_eq!(s.average_run_duration_ms, None);
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn telemetry_summary_counts_by_type() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    c.record(mk_event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.events_by_type["run_started"], 1);
    assert_eq!(s.events_by_type["run_completed"], 2);
    assert_eq!(s.events_by_type["run_failed"], 1);
}

#[test]
fn telemetry_summary_average_run_duration() {
    let mut c = TelemetryCollector::new();
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(150));
}

#[test]
fn telemetry_summary_error_rate_computation() {
    let mut c = TelemetryCollector::new();
    // 2 completed, 1 failed => error_rate = 1/3
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    c.record(mk_event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    assert!((s.error_rate - 1.0 / 3.0).abs() < 0.01);
}

// =========================================================================
// Additional: MetricsCollector summary edge cases
// =========================================================================

#[test]
fn metrics_collector_summary_single_run() {
    let c = MetricsCollector::new();
    c.record(mk_run("only", 77, 10, 20, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, 77.0);
    assert_eq!(s.p50_duration_ms, 77.0);
    assert_eq!(s.p99_duration_ms, 77.0);
}

#[test]
fn metrics_collector_summary_all_errors() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 10, 5, 5, 3));
    c.record(mk_run("b", 20, 5, 5, 7));
    let s = c.summary();
    assert!((s.error_rate - 5.0).abs() < f64::EPSILON); // (3+7)/2 = 5
}

#[test]
fn metrics_collector_large_batch() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(mk_run("bulk", i, 1, 1, 0));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    assert_eq!(s.total_tokens_in, 1000);
    assert_eq!(s.total_tokens_out, 1000);
}

#[test]
fn request_counter_snapshot_deterministic_order() {
    let c = RequestCounter::new();
    c.increment("z_backend", "z_dialect", "ok");
    c.increment("a_backend", "a_dialect", "ok");
    let snap = c.snapshot();
    let keys: Vec<_> = snap.keys().collect();
    assert!(keys[0].backend < keys[1].backend);
}

#[test]
fn error_counter_snapshot_deterministic_order() {
    let c = ErrorCounter::new();
    c.increment("Z_ERR");
    c.increment("A_ERR");
    let snap = c.snapshot();
    let keys: Vec<_> = snap.keys().collect();
    assert!(keys[0] < keys[1]);
}
