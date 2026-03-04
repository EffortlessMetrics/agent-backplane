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
// Comprehensive telemetry tests — agent-185.
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
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

fn make_event(
    event_type: TelemetryEventType,
    run_id: Option<&str>,
    backend: Option<&str>,
    duration_ms: Option<u64>,
) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".into(),
        event_type,
        run_id: run_id.map(Into::into),
        backend: backend.map(Into::into),
        metadata: BTreeMap::new(),
        duration_ms,
    }
}

// =========================================================================
//  1. Metrics — counter increment, gauge set, histogram record (12 tests)
// =========================================================================

#[test]
fn request_counter_increment_distinct_keys() {
    let c = RequestCounter::new();
    c.increment("mock", "openai", "success");
    c.increment("mock", "openai", "error");
    c.increment("sidecar", "anthropic", "success");
    assert_eq!(c.get("mock", "openai", "success"), 1);
    assert_eq!(c.get("mock", "openai", "error"), 1);
    assert_eq!(c.get("sidecar", "anthropic", "success"), 1);
    assert_eq!(c.total(), 3);
}

#[test]
fn request_counter_repeated_increment() {
    let c = RequestCounter::new();
    for _ in 0..100 {
        c.increment("b", "d", "ok");
    }
    assert_eq!(c.get("b", "d", "ok"), 100);
    assert_eq!(c.total(), 100);
}

#[test]
fn request_counter_snapshot_deterministic_order() {
    let c = RequestCounter::new();
    c.increment("z-backend", "z-dialect", "ok");
    c.increment("a-backend", "a-dialect", "ok");
    let snap = c.snapshot();
    let keys: Vec<_> = snap.keys().collect();
    assert!(
        keys[0].backend < keys[1].backend,
        "BTreeMap keys are sorted"
    );
}

#[test]
fn request_counter_reset_clears_all() {
    let c = RequestCounter::new();
    c.increment("a", "b", "c");
    c.increment("d", "e", "f");
    c.reset();
    assert_eq!(c.total(), 0);
    assert!(c.snapshot().is_empty());
}

#[test]
fn request_counter_get_missing_returns_zero() {
    let c = RequestCounter::new();
    assert_eq!(c.get("nonexistent", "none", "nope"), 0);
}

#[test]
fn error_counter_multiple_codes() {
    let c = ErrorCounter::new();
    c.increment("timeout");
    c.increment("timeout");
    c.increment("rate_limit");
    c.increment("auth_failure");
    assert_eq!(c.get("timeout"), 2);
    assert_eq!(c.get("rate_limit"), 1);
    assert_eq!(c.get("auth_failure"), 1);
    assert_eq!(c.total(), 4);
}

#[test]
fn error_counter_snapshot_and_reset() {
    let c = ErrorCounter::new();
    c.increment("E001");
    c.increment("E002");
    let snap = c.snapshot();
    assert_eq!(snap.len(), 2);
    c.reset();
    assert_eq!(c.total(), 0);
    assert!(c.snapshot().is_empty());
}

#[test]
fn gauge_increment_decrement_below_zero() {
    let g = ActiveRequestGauge::new();
    assert_eq!(g.get(), 0);
    g.decrement();
    assert_eq!(g.get(), -1);
    g.decrement();
    assert_eq!(g.get(), -2);
    g.increment();
    assert_eq!(g.get(), -1);
}

#[test]
fn gauge_many_increments_decrements() {
    let g = ActiveRequestGauge::new();
    for _ in 0..50 {
        g.increment();
    }
    assert_eq!(g.get(), 50);
    for _ in 0..30 {
        g.decrement();
    }
    assert_eq!(g.get(), 20);
}

#[test]
fn token_accumulator_add_multiple_times() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.add(50, 75);
    t.add(25, 25);
    assert_eq!(t.total_input(), 175);
    assert_eq!(t.total_output(), 300);
    assert_eq!(t.total(), 475);
}

#[test]
fn token_accumulator_reset_zeroes_both() {
    let t = TokenAccumulator::new();
    t.add(1000, 2000);
    t.reset();
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
    assert_eq!(t.total(), 0);
}

#[test]
fn token_accumulator_zero_add_is_noop() {
    let t = TokenAccumulator::new();
    t.add(0, 0);
    assert_eq!(t.total(), 0);
}

// =========================================================================
//  2. Spans — creation, attributes, nesting (7 tests)
// =========================================================================

#[test]
fn span_request_span_does_not_panic() {
    let span = spans::request_span("wo-123", "refactor auth", "mapped");
    let _guard = span.enter();
}

#[test]
fn span_event_span_does_not_panic() {
    let span = spans::event_span("tool_call", 42);
    let _guard = span.enter();
}

#[test]
fn span_backend_span_does_not_panic() {
    let span = spans::backend_span("sidecar:node");
    let _guard = span.enter();
}

#[test]
fn span_nested_request_event() {
    let parent = spans::request_span("wo-1", "task", "mapped");
    let _pg = parent.enter();
    let child = spans::event_span("tool_call", 1);
    let _cg = child.enter();
    // No panic means nesting works.
}

#[test]
fn telemetry_span_builder_attributes() {
    let span = TelemetrySpan::new("my_op")
        .with_attribute("key1", "val1")
        .with_attribute("key2", "val2")
        .with_attribute("key3", "val3");
    assert_eq!(span.name, "my_op");
    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["key1"], "val1");
}

#[test]
fn telemetry_span_overwrite_attribute() {
    let span = TelemetrySpan::new("op")
        .with_attribute("k", "v1")
        .with_attribute("k", "v2");
    assert_eq!(span.attributes.len(), 1);
    assert_eq!(span.attributes["k"], "v2");
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("run")
        .with_attribute("backend", "mock")
        .with_attribute("lane", "mapped");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "run");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["lane"], "mapped");
}

// =========================================================================
//  3. Hooks — event hooks, lifecycle hooks (8 tests)
// =========================================================================

#[test]
fn hook_request_start_returns_instant() {
    let before = Instant::now();
    let start = on_request_start("wo-1", "mock");
    assert!(start >= before);
}

#[test]
fn hook_request_complete_success_returns_elapsed() {
    let start = Instant::now();
    let elapsed = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
    assert!(elapsed < 5000);
}

#[test]
fn hook_request_complete_error_returns_elapsed() {
    let start = Instant::now();
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    let elapsed = on_request_complete("wo-1", "mock", &outcome, start);
    assert!(elapsed < 5000);
}

#[test]
fn hook_on_error_all_classifications() {
    on_error(
        "wo-1",
        "E001",
        "transient err",
        ErrorClassification::Transient,
    );
    on_error(
        "wo-2",
        "E002",
        "permanent err",
        ErrorClassification::Permanent,
    );
    on_error("wo-3", "E003", "unknown err", ErrorClassification::Unknown);
}

#[test]
fn hook_error_classification_display() {
    assert_eq!(ErrorClassification::Transient.to_string(), "transient");
    assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
    assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
}

#[test]
fn hook_request_outcome_equality() {
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
fn hook_request_outcome_clone() {
    let outcome = RequestOutcome::Error {
        code: "rate_limit".into(),
        message: "429".into(),
    };
    let cloned = outcome.clone();
    assert_eq!(outcome, cloned);
}

#[test]
fn hook_error_classification_copy() {
    let c = ErrorClassification::Transient;
    let c2 = c;
    assert_eq!(c, c2);
}

// =========================================================================
//  4. Pipeline — TelemetryCollector, filter, summary (10 tests)
// =========================================================================

#[test]
fn pipeline_collector_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.error_rate, 0.0);
    assert!(s.average_run_duration_ms.is_none());
}

#[test]
fn pipeline_collector_records_events() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        Some("mock"),
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        Some("mock"),
        Some(100),
    ));
    assert_eq!(c.events().len(), 2);
}

#[test]
fn pipeline_collector_events_of_type() {
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
        Some(50),
    ));
    c.record(make_event(
        TelemetryEventType::RunStarted,
        Some("r2"),
        None,
        None,
    ));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
    let completed = c.events_of_type(TelemetryEventType::RunCompleted);
    assert_eq!(completed.len(), 1);
}

#[test]
fn pipeline_collector_run_events() {
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
        Some(50),
    ));
    c.record(make_event(
        TelemetryEventType::RunFailed,
        Some("r1"),
        None,
        None,
    ));
    let r1_events = c.run_events("r1");
    assert_eq!(r1_events.len(), 2);
    let r2_events = c.run_events("r2");
    assert_eq!(r2_events.len(), 1);
}

#[test]
fn pipeline_collector_summary_avg_duration() {
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
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(150));
}

#[test]
fn pipeline_collector_summary_error_rate() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(100),
    ));
    c.record(make_event(
        TelemetryEventType::RunFailed,
        Some("r2"),
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r3"),
        None,
        Some(200),
    ));
    let s = c.summary();
    // 1 failed / (2 completed + 1 failed) = 1/3
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn pipeline_filter_allowed_types() {
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
    c.record(make_event(TelemetryEventType::RunFailed, None, None, None));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].event_type, TelemetryEventType::RunCompleted);
}

#[test]
fn pipeline_filter_min_duration() {
    let filter = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(50),
    };
    let mut c = TelemetryCollector::with_filter(filter);
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(10),
    ));
    c.record(make_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    // Events without duration pass through (not filtered by duration).
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    assert_eq!(c.events().len(), 2);
}

#[test]
fn pipeline_collector_clear() {
    let mut c = TelemetryCollector::new();
    c.record(make_event(TelemetryEventType::RunStarted, None, None, None));
    c.clear();
    assert!(c.events().is_empty());
}

#[test]
fn pipeline_event_type_display() {
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
        TelemetryEventType::MappingPerformed.to_string(),
        "mapping_performed"
    );
}

// =========================================================================
//  5. Thread safety — concurrent metric updates (6 tests)
// =========================================================================

#[test]
fn thread_safety_request_counter_concurrent() {
    let c = RequestCounter::new();
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                for _ in 0..100 {
                    cc.increment("b", "d", "ok");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.total(), 1000);
}

#[test]
fn thread_safety_error_counter_concurrent() {
    let c = ErrorCounter::new();
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let cc = c.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    cc.increment("E001");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.get("E001"), 500);
}

#[test]
fn thread_safety_gauge_concurrent_inc_dec() {
    let g = Arc::new(ActiveRequestGauge::new());
    let mut handles = Vec::new();
    for _ in 0..10 {
        let gg = Arc::clone(&g);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                gg.increment();
            }
        }));
    }
    for _ in 0..10 {
        let gg = Arc::clone(&g);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                gg.decrement();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(g.get(), 0);
}

#[test]
fn thread_safety_token_accumulator_concurrent() {
    let t = Arc::new(TokenAccumulator::new());
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let tt = Arc::clone(&t);
            thread::spawn(move || {
                for _ in 0..100 {
                    tt.add(1, 2);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(t.total_input(), 1000);
    assert_eq!(t.total_output(), 2000);
    assert_eq!(t.total(), 3000);
}

#[test]
fn thread_safety_collector_concurrent_record() {
    let c = MetricsCollector::new();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let cc = c.clone();
            thread::spawn(move || {
                cc.record(run("b", "d", i * 10, 1, 1, 0));
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
    assert_eq!(c.summary().count, 20);
}

#[test]
fn thread_safety_collector_concurrent_summary_while_writing() {
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

// =========================================================================
//  6. Serialization — metrics export, JSON format (8 tests)
// =========================================================================

#[test]
fn serde_run_metrics_roundtrip() {
    let m = run("serde_test", "openai", 999, 50, 100, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn serde_metrics_summary_roundtrip() {
    let s = make_summary(5, 100.0, 90.0, 200.0);
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn serde_export_json_valid() {
    let s = make_summary(3, 50.0, 45.0, 99.0);
    let json = MetricsExporter::export_json(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 3);
}

#[test]
fn serde_export_csv_header_and_rows() {
    let runs = vec![
        run("alpha", "openai", 100, 50, 80, 0),
        run("beta", "anthropic", 200, 60, 90, 1),
    ];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("backend_name,"));
    assert!(lines[1].starts_with("alpha,"));
}

#[test]
fn serde_export_structured_all_keys() {
    let s = make_summary(10, 75.0, 70.0, 150.0);
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count=10"));
    assert!(out.contains("mean_duration_ms=75.00"));
    assert!(out.contains("error_rate=0.0500"));
    assert!(out.contains("backend.mock=10"));
}

#[test]
fn serde_export_format_dispatch() {
    let s = make_summary(1, 10.0, 10.0, 10.0);
    let json_out = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    serde_json::from_str::<serde_json::Value>(&json_out).unwrap();
    let csv_out = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(csv_out.contains("count,mean_duration_ms"));
    let struct_out = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(struct_out.contains("count=1"));
}

#[test]
fn serde_export_format_enum_roundtrip() {
    for fmt in [
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Structured,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let d: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(d, fmt);
    }
}

#[test]
fn serde_json_exporter_trait_object() {
    let exporter: Box<dyn TelemetryExporter> = Box::new(JsonExporter);
    let s = MetricsSummary::default();
    let json = exporter.export(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["count"], 0);
}

// =========================================================================
//  7. Configuration — metric names, labels, buckets (6 tests)
// =========================================================================

#[test]
fn config_histogram_custom_buckets() {
    let mut h = LatencyHistogram::new();
    for v in [1.0, 5.0, 15.0, 50.0, 150.0, 500.0, 1000.0] {
        h.record(v);
    }
    let buckets = h.buckets(&[10.0, 100.0, 500.0]);
    // [0,10)=2, [10,100)=2, [100,500)=1, [500,∞)=2
    assert_eq!(buckets, vec![2, 2, 1, 2]);
}

#[test]
fn config_histogram_single_bucket_boundary() {
    let mut h = LatencyHistogram::new();
    h.record(5.0);
    h.record(50.0);
    let buckets = h.buckets(&[10.0]);
    assert_eq!(buckets, vec![1, 1]);
}

#[test]
fn config_backend_counts_btreemap_ordering() {
    let c = MetricsCollector::new();
    c.record(run("zebra", "d", 10, 1, 1, 0));
    c.record(run("alpha", "d", 20, 1, 1, 0));
    c.record(run("middle", "d", 30, 1, 1, 0));
    let s = c.summary();
    let keys: Vec<_> = s.backend_counts.keys().cloned().collect();
    assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn config_run_summary_json_event_key_order() {
    let s = RunSummary::from_events(&["z_event", "a_event", "m_event"], 0);
    let json = serde_json::to_string(&s).unwrap();
    let a_pos = json.find("a_event").unwrap();
    let m_pos = json.find("m_event").unwrap();
    let z_pos = json.find("z_event").unwrap();
    assert!(a_pos < m_pos && m_pos < z_pos);
}

#[test]
fn config_request_key_serde_roundtrip() {
    let key = RequestKey {
        backend: "sidecar:node".into(),
        dialect: "openai".into(),
        outcome: "success".into(),
    };
    let json = serde_json::to_string(&key).unwrap();
    let key2: RequestKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, key2);
}

#[test]
fn config_model_pricing_serde_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.00003,
        output_cost_per_token: 0.00006,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

// =========================================================================
//  8. Edge cases — overflow, negative values, empty names (8 tests)
// =========================================================================

#[test]
fn edge_empty_backend_name() {
    let c = MetricsCollector::new();
    c.record(run("", "", 100, 10, 20, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.backend_counts[""], 1);
}

#[test]
fn edge_histogram_zero_values() {
    let mut h = LatencyHistogram::new();
    h.record(0.0);
    h.record(0.0);
    h.record(0.0);
    assert_eq!(h.mean(), 0.0);
    assert_eq!(h.p50(), 0.0);
    assert_eq!(h.min(), Some(0.0));
    assert_eq!(h.max(), Some(0.0));
}

#[test]
fn edge_histogram_negative_values() {
    let mut h = LatencyHistogram::new();
    h.record(-5.0);
    h.record(-10.0);
    h.record(0.0);
    assert_eq!(h.min(), Some(-10.0));
    assert_eq!(h.max(), Some(0.0));
    assert!((h.mean() - (-5.0)).abs() < f64::EPSILON);
}

#[test]
fn edge_histogram_very_large_values() {
    let mut h = LatencyHistogram::new();
    h.record(1e15);
    h.record(2e15);
    assert!((h.mean() - 1.5e15).abs() < 1.0);
    assert_eq!(h.min(), Some(1e15));
    assert_eq!(h.max(), Some(2e15));
}

#[test]
fn edge_run_summary_empty_string_event() {
    let mut s = RunSummary::new();
    s.record_event("");
    assert_eq!(s.total_events, 1);
    assert_eq!(*s.event_counts.get("").unwrap(), 1);
    // Empty-string event is not error/warning/tool_call.
    assert_eq!(s.error_count, 0);
}

#[test]
fn edge_cost_estimator_very_large_tokens() {
    let mut ce = CostEstimator::new();
    ce.set_pricing(
        "m",
        ModelPricing {
            input_cost_per_token: 0.001,
            output_cost_per_token: 0.002,
        },
    );
    let cost = ce.estimate("m", u64::MAX / 2, 0).unwrap();
    assert!(cost > 0.0);
}

#[test]
fn edge_collector_record_max_duration() {
    let c = MetricsCollector::new();
    c.record(run("b", "d", u64::MAX, 0, 0, 0));
    let s = c.summary();
    assert_eq!(s.count, 1);
    assert_eq!(s.mean_duration_ms, u64::MAX as f64);
}

#[test]
fn edge_run_metrics_default_all_zero() {
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

// =========================================================================
//  9. Integration — metrics + spans + pipeline together (10 tests)
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
fn integration_hooks_with_metrics_collector() {
    let c = MetricsCollector::new();
    let start = on_request_start("wo-1", "mock");
    let elapsed = on_request_complete("wo-1", "mock", &RequestOutcome::Success, start);
    c.record(RunMetrics {
        backend_name: "mock".into(),
        dialect: "test".into(),
        duration_ms: elapsed,
        events_count: 0,
        tokens_in: 0,
        tokens_out: 0,
        tool_calls_count: 0,
        errors_count: 0,
        emulations_applied: 0,
    });
    assert_eq!(c.len(), 1);
}

#[test]
fn integration_pipeline_collector_with_request_counter() {
    let req_counter = RequestCounter::new();
    let mut pipeline = TelemetryCollector::new();

    req_counter.increment("mock", "openai", "success");
    pipeline.record(make_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        Some("mock"),
        Some(100),
    ));

    req_counter.increment("mock", "openai", "error");
    pipeline.record(make_event(
        TelemetryEventType::RunFailed,
        Some("r2"),
        Some("mock"),
        None,
    ));

    assert_eq!(req_counter.total(), 2);
    assert_eq!(pipeline.events().len(), 2);
    let ps = pipeline.summary();
    assert!((ps.error_rate - 0.5).abs() < 1e-10);
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
fn integration_run_summary_merge_multiple() {
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
fn integration_large_scale_summary_stable() {
    let c = MetricsCollector::new();
    for i in 0..1000 {
        c.record(run("b", "d", i, i, i * 2, if i % 100 == 0 { 1 } else { 0 }));
    }
    let s = c.summary();
    assert_eq!(s.count, 1000);
    assert!((s.mean_duration_ms - 499.5).abs() < f64::EPSILON);
    assert!(s.p50_duration_ms > 0.0);
    assert!(s.p99_duration_ms > s.p50_duration_ms);
    assert!((s.error_rate - 0.01).abs() < f64::EPSILON);
}

#[test]
fn integration_cost_from_collector_runs() {
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
fn integration_collector_clear_then_reuse() {
    let c = MetricsCollector::new();
    c.record(run("a", "d", 50, 5, 5, 0));
    c.clear();
    assert!(c.is_empty());
    c.record(run("b", "d", 100, 10, 10, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.summary().backend_counts["b"], 1);
}
