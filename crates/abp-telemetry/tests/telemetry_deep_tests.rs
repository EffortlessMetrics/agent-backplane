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
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the abp-telemetry crate covering collector, filter, summary,
//! pipeline composition, serde roundtrips, edge cases, and metrics aggregation.

use abp_telemetry::pipeline::*;
use abp_telemetry::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(
    event_type: TelemetryEventType,
    ts: &str,
    run_id: Option<&str>,
    backend: Option<&str>,
    duration_ms: Option<u64>,
) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: ts.to_string(),
        event_type,
        run_id: run_id.map(String::from),
        backend: backend.map(String::from),
        metadata: BTreeMap::new(),
        duration_ms,
    }
}

fn ev(event_type: TelemetryEventType) -> TelemetryEvent {
    make_event(event_type, "2025-01-01T00:00:00Z", None, None, None)
}

fn ev_run(event_type: TelemetryEventType, run_id: &str) -> TelemetryEvent {
    make_event(event_type, "2025-01-01T00:00:00Z", Some(run_id), None, None)
}

fn ev_backend(event_type: TelemetryEventType, backend: &str) -> TelemetryEvent {
    make_event(
        event_type,
        "2025-01-01T00:00:00Z",
        None,
        Some(backend),
        None,
    )
}

// ===========================================================================
// TelemetryCollector: query events by backend
// ===========================================================================

#[test]
fn collector_events_filtered_by_backend() {
    let mut c = TelemetryCollector::new();
    c.record(ev_backend(TelemetryEventType::BackendSelected, "openai"));
    c.record(ev_backend(TelemetryEventType::BackendSelected, "anthropic"));
    c.record(ev_backend(TelemetryEventType::BackendSelected, "openai"));
    let openai_events: Vec<_> = c
        .events()
        .iter()
        .filter(|e| e.backend.as_deref() == Some("openai"))
        .collect();
    assert_eq!(openai_events.len(), 2);
}

fn ev_dur(event_type: TelemetryEventType, duration_ms: u64) -> TelemetryEvent {
    make_event(
        event_type,
        "2025-01-01T00:00:00Z",
        None,
        None,
        Some(duration_ms),
    )
}

fn ev_full(
    event_type: TelemetryEventType,
    ts: &str,
    run_id: &str,
    backend: &str,
    duration_ms: u64,
) -> TelemetryEvent {
    make_event(
        event_type,
        ts,
        Some(run_id),
        Some(backend),
        Some(duration_ms),
    )
}

fn simple_run(backend: &str, duration_ms: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.to_string(),
        dialect: "test".to_string(),
        duration_ms,
        events_count: 1,
        tokens_in: 100,
        tokens_out: 200,
        tool_calls_count: 0,
        errors_count: errors,
        emulations_applied: 0,
    }
}

// ===========================================================================
// 1. TelemetryCollector: create, record, query
// ===========================================================================

#[test]
fn collector_create_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
}

#[test]
fn collector_default_is_empty() {
    let c = TelemetryCollector::default();
    assert!(c.events().is_empty());
}

#[test]
fn collector_record_single_event() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    assert_eq!(c.events().len(), 1);
}

#[test]
fn collector_record_multiple_events() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::BackendSelected));
    c.record(ev(TelemetryEventType::RunCompleted));
    assert_eq!(c.events().len(), 3);
}

#[test]
fn collector_query_by_type_run_started() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::RunFailed));
    c.record(ev(TelemetryEventType::RunStarted));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
}

#[test]
fn collector_query_by_type_returns_empty_for_absent_type() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    assert!(c
        .events_of_type(TelemetryEventType::FallbackTriggered)
        .is_empty());
}

#[test]
fn collector_run_events_filter_by_run_id() {
    let mut c = TelemetryCollector::new();
    c.record(ev_run(TelemetryEventType::RunStarted, "run-1"));
    c.record(ev_run(TelemetryEventType::RunCompleted, "run-1"));
    c.record(ev_run(TelemetryEventType::RunStarted, "run-2"));
    let r1 = c.run_events("run-1");
    assert_eq!(r1.len(), 2);
    assert!(r1.iter().all(|e| e.run_id.as_deref() == Some("run-1")));
}

#[test]
fn collector_run_events_no_match() {
    let mut c = TelemetryCollector::new();
    c.record(ev_run(TelemetryEventType::RunStarted, "run-1"));
    assert!(c.run_events("nonexistent").is_empty());
}

#[test]
fn collector_clear_removes_all() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::RunFailed));
    c.clear();
    assert!(c.events().is_empty());
    assert_eq!(c.summary().total_events, 0);
}

// ===========================================================================
// 2. TelemetryFilter: filter events by type and duration
// ===========================================================================

#[test]
fn filter_default_matches_everything() {
    let f = TelemetryFilter::default();
    assert!(f.matches(&ev(TelemetryEventType::RunStarted)));
    assert!(f.matches(&ev(TelemetryEventType::RunFailed)));
    assert!(f.matches(&ev_dur(TelemetryEventType::RunCompleted, 500)));
}

#[test]
fn filter_by_allowed_types_accepts_listed() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![
            TelemetryEventType::RunStarted,
            TelemetryEventType::RunCompleted,
        ]),
        min_duration_ms: None,
    };
    assert!(f.matches(&ev(TelemetryEventType::RunStarted)));
    assert!(f.matches(&ev(TelemetryEventType::RunCompleted)));
}

#[test]
fn filter_by_allowed_types_rejects_unlisted() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    assert!(!f.matches(&ev(TelemetryEventType::RunFailed)));
    assert!(!f.matches(&ev(TelemetryEventType::BackendSelected)));
}

#[test]
fn filter_min_duration_rejects_below_threshold() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    assert!(!f.matches(&ev_dur(TelemetryEventType::RunCompleted, 50)));
}

#[test]
fn filter_min_duration_accepts_at_threshold() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    assert!(f.matches(&ev_dur(TelemetryEventType::RunCompleted, 100)));
}

#[test]
fn filter_min_duration_accepts_above_threshold() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    assert!(f.matches(&ev_dur(TelemetryEventType::RunCompleted, 200)));
}

#[test]
fn filter_min_duration_passes_events_without_duration() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    // Events with no duration_ms are not filtered by the duration predicate.
    assert!(f.matches(&ev(TelemetryEventType::RunStarted)));
}

#[test]
fn filter_combined_type_and_duration() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(50),
    };
    // Passes both predicates.
    assert!(f.matches(&ev_dur(TelemetryEventType::RunCompleted, 100)));
    // Fails duration.
    assert!(!f.matches(&ev_dur(TelemetryEventType::RunCompleted, 10)));
    // Fails type.
    assert!(!f.matches(&ev_dur(TelemetryEventType::RunFailed, 100)));
}

// ===========================================================================
// 3. TelemetrySummary: aggregate statistics
// ===========================================================================

#[test]
fn summary_empty_collector_defaults() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert!(s.average_run_duration_ms.is_none());
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn summary_counts_events_by_type() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::BackendSelected));
    c.record(ev(TelemetryEventType::MappingPerformed));
    let s = c.summary();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.events_by_type["run_started"], 2);
    assert_eq!(s.events_by_type["backend_selected"], 1);
    assert_eq!(s.events_by_type["mapping_performed"], 1);
}

#[test]
fn summary_average_run_duration_from_completed() {
    let mut c = TelemetryCollector::new();
    c.record(ev_dur(TelemetryEventType::RunCompleted, 100));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 200));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 300));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(200));
}

#[test]
fn summary_average_duration_ignores_non_completed_events() {
    let mut c = TelemetryCollector::new();
    // RunStarted with duration should be ignored for average calculation.
    c.record(ev_dur(TelemetryEventType::RunStarted, 999));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 100));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(100));
}

#[test]
fn summary_no_completed_events_yields_none_duration() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::RunFailed));
    let s = c.summary();
    assert!(s.average_run_duration_ms.is_none());
}

#[test]
fn summary_error_rate_mixed() {
    let mut c = TelemetryCollector::new();
    c.record(ev_dur(TelemetryEventType::RunCompleted, 50));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 60));
    c.record(ev(TelemetryEventType::RunFailed));
    let s = c.summary();
    // 1 failed / (2 completed + 1 failed) = 1/3.
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn summary_error_rate_all_failed() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunFailed));
    c.record(ev(TelemetryEventType::RunFailed));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < 1e-9);
}

#[test]
fn summary_error_rate_zero_when_no_outcomes() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::BackendSelected));
    let s = c.summary();
    assert!((s.error_rate - 0.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 4. Event types: all variants
// ===========================================================================

#[test]
fn all_event_types_display_snake_case() {
    let cases = vec![
        (TelemetryEventType::RunStarted, "run_started"),
        (TelemetryEventType::RunCompleted, "run_completed"),
        (TelemetryEventType::RunFailed, "run_failed"),
        (TelemetryEventType::BackendSelected, "backend_selected"),
        (TelemetryEventType::RetryAttempted, "retry_attempted"),
        (TelemetryEventType::FallbackTriggered, "fallback_triggered"),
        (
            TelemetryEventType::CapabilityNegotiated,
            "capability_negotiated",
        ),
        (TelemetryEventType::MappingPerformed, "mapping_performed"),
    ];
    for (variant, expected) in cases {
        assert_eq!(variant.to_string(), expected);
    }
}

#[test]
fn all_event_types_recorded_in_collector() {
    let mut c = TelemetryCollector::new();
    let types = vec![
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunCompleted,
        TelemetryEventType::RunFailed,
        TelemetryEventType::BackendSelected,
        TelemetryEventType::RetryAttempted,
        TelemetryEventType::FallbackTriggered,
        TelemetryEventType::CapabilityNegotiated,
        TelemetryEventType::MappingPerformed,
    ];
    for t in &types {
        c.record(ev(t.clone()));
    }
    assert_eq!(c.events().len(), 8);
    let s = c.summary();
    assert_eq!(s.events_by_type.len(), 8);
}

#[test]
fn event_type_equality_same() {
    assert_eq!(
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunStarted
    );
}

#[test]
fn event_type_inequality_different() {
    assert_ne!(
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunFailed
    );
}

// ===========================================================================
// 5. Pipeline composition: collector → filter → summary
// ===========================================================================

#[test]
fn pipeline_filter_then_summary() {
    let filter = TelemetryFilter {
        allowed_types: Some(vec![
            TelemetryEventType::RunCompleted,
            TelemetryEventType::RunFailed,
        ]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(filter);

    // Feed a mix of events.
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 100));
    c.record(ev(TelemetryEventType::BackendSelected));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 200));
    c.record(ev(TelemetryEventType::RunFailed));

    // Only RunCompleted and RunFailed should be recorded.
    assert_eq!(c.events().len(), 3);
    let s = c.summary();
    assert_eq!(s.total_events, 3);
    assert_eq!(s.average_run_duration_ms, Some(150));
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn pipeline_duration_filter_then_summary() {
    let filter = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(150),
    };
    let mut c = TelemetryCollector::with_filter(filter);

    c.record(ev_dur(TelemetryEventType::RunCompleted, 100));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 200));
    c.record(ev_dur(TelemetryEventType::RunCompleted, 300));
    // Events without duration pass through the duration filter.
    c.record(ev(TelemetryEventType::RunStarted));

    // 100ms is below threshold, so rejected. 200, 300, and RunStarted pass.
    assert_eq!(c.events().len(), 3);
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(250));
}

#[test]
fn pipeline_multiple_runs_summary() {
    let mut c = TelemetryCollector::new();

    // Run 1: success.
    c.record(ev_run(TelemetryEventType::RunStarted, "r1"));
    c.record(ev_full(
        TelemetryEventType::RunCompleted,
        "2025-01-01T00:01:00Z",
        "r1",
        "mock",
        500,
    ));

    // Run 2: failure.
    c.record(ev_run(TelemetryEventType::RunStarted, "r2"));
    c.record(ev_run(TelemetryEventType::RunFailed, "r2"));

    let s = c.summary();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.average_run_duration_ms, Some(500));
    // 1 fail / (1 completed + 1 failed) = 0.5
    assert!((s.error_rate - 0.5).abs() < 1e-9);
}

// ===========================================================================
// 6. Timestamp handling: events ordered by timestamp
// ===========================================================================

#[test]
fn events_preserve_insertion_order() {
    let mut c = TelemetryCollector::new();
    let ts = [
        "2025-01-01T00:00:01Z",
        "2025-01-01T00:00:02Z",
        "2025-01-01T00:00:03Z",
    ];
    for t in &ts {
        c.record(make_event(
            TelemetryEventType::RunStarted,
            t,
            None,
            None,
            None,
        ));
    }
    let events = c.events();
    assert_eq!(events[0].timestamp, ts[0]);
    assert_eq!(events[1].timestamp, ts[1]);
    assert_eq!(events[2].timestamp, ts[2]);
}

#[test]
fn events_sortable_by_timestamp() {
    let mut c = TelemetryCollector::new();
    // Insert out of order.
    c.record(make_event(
        TelemetryEventType::RunStarted,
        "2025-01-01T00:00:03Z",
        None,
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunStarted,
        "2025-01-01T00:00:01Z",
        None,
        None,
        None,
    ));
    c.record(make_event(
        TelemetryEventType::RunStarted,
        "2025-01-01T00:00:02Z",
        None,
        None,
        None,
    ));

    let mut sorted: Vec<_> = c.events().to_vec();
    sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    assert_eq!(sorted[0].timestamp, "2025-01-01T00:00:01Z");
    assert_eq!(sorted[1].timestamp, "2025-01-01T00:00:02Z");
    assert_eq!(sorted[2].timestamp, "2025-01-01T00:00:03Z");
}

#[test]
fn events_with_identical_timestamps_all_preserved() {
    let mut c = TelemetryCollector::new();
    let ts = "2025-01-01T00:00:00Z";
    for _ in 0..5 {
        c.record(make_event(
            TelemetryEventType::RunStarted,
            ts,
            None,
            None,
            None,
        ));
    }
    assert_eq!(c.events().len(), 5);
}

// ===========================================================================
// 7. Context propagation: trace/span IDs flow through metadata
// ===========================================================================

#[test]
fn trace_id_in_metadata_survives_collection() {
    let mut c = TelemetryCollector::new();
    let mut meta = BTreeMap::new();
    meta.insert("trace_id".to_string(), serde_json::json!("abc-123-trace"));
    meta.insert("span_id".to_string(), serde_json::json!("span-456"));
    let event = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::RunStarted,
        run_id: Some("r1".to_string()),
        backend: None,
        metadata: meta,
        duration_ms: None,
    };
    c.record(event);
    let stored = &c.events()[0];
    assert_eq!(stored.metadata["trace_id"], "abc-123-trace");
    assert_eq!(stored.metadata["span_id"], "span-456");
}

#[test]
fn trace_id_preserved_after_serde_roundtrip() {
    let mut meta = BTreeMap::new();
    meta.insert("trace_id".to_string(), serde_json::json!("trace-roundtrip"));
    meta.insert("span_id".to_string(), serde_json::json!("span-rt"));
    let event = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::BackendSelected,
        run_id: None,
        backend: Some("mock".to_string()),
        metadata: meta,
        duration_ms: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.metadata["trace_id"], "trace-roundtrip");
    assert_eq!(deserialized.metadata["span_id"], "span-rt");
}

#[test]
fn multiple_events_share_trace_id() {
    let mut c = TelemetryCollector::new();
    for etype in &[
        TelemetryEventType::RunStarted,
        TelemetryEventType::BackendSelected,
        TelemetryEventType::RunCompleted,
    ] {
        let mut meta = BTreeMap::new();
        meta.insert("trace_id".to_string(), serde_json::json!("shared-trace"));
        c.record(TelemetryEvent {
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_type: etype.clone(),
            run_id: Some("r1".to_string()),
            backend: None,
            metadata: meta,
            duration_ms: None,
        });
    }
    let events = c.run_events("r1");
    assert_eq!(events.len(), 3);
    for e in &events {
        assert_eq!(e.metadata["trace_id"], "shared-trace");
    }
}

// ===========================================================================
// 8. Serde roundtrip: all telemetry types
// ===========================================================================

#[test]
fn telemetry_event_serde_roundtrip() {
    let e = ev_full(
        TelemetryEventType::RunCompleted,
        "2025-06-01T12:00:00Z",
        "run-42",
        "openai",
        1500,
    );
    let json = serde_json::to_string(&e).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.event_type, TelemetryEventType::RunCompleted);
    assert_eq!(e2.run_id.as_deref(), Some("run-42"));
    assert_eq!(e2.backend.as_deref(), Some("openai"));
    assert_eq!(e2.duration_ms, Some(1500));
}

#[test]
fn telemetry_event_type_serde_all_variants() {
    let variants = vec![
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunCompleted,
        TelemetryEventType::RunFailed,
        TelemetryEventType::BackendSelected,
        TelemetryEventType::RetryAttempted,
        TelemetryEventType::FallbackTriggered,
        TelemetryEventType::CapabilityNegotiated,
        TelemetryEventType::MappingPerformed,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let v2: TelemetryEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }
}

#[test]
fn telemetry_filter_serde_roundtrip() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![
            TelemetryEventType::RunStarted,
            TelemetryEventType::RunFailed,
        ]),
        min_duration_ms: Some(42),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: TelemetryFilter = serde_json::from_str(&json).unwrap();
    assert_eq!(f2.min_duration_ms, Some(42));
    let types = f2.allowed_types.unwrap();
    assert_eq!(types.len(), 2);
    assert!(types.contains(&TelemetryEventType::RunStarted));
    assert!(types.contains(&TelemetryEventType::RunFailed));
}

#[test]
fn telemetry_summary_serde_roundtrip() {
    let mut c = TelemetryCollector::new();
    c.record(ev_dur(TelemetryEventType::RunCompleted, 100));
    c.record(ev(TelemetryEventType::RunFailed));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: TelemetrySummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s2.total_events, 2);
    assert_eq!(s2.average_run_duration_ms, Some(100));
}

#[test]
fn run_metrics_serde_roundtrip() {
    let m = simple_run("serde-test", 999, 2);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("b", 200, 1));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("operation")
        .with_attribute("backend", "mock")
        .with_attribute("run_id", "r1");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "operation");
    assert_eq!(span2.attributes["backend"], "mock");
    assert_eq!(span2.attributes["run_id"], "r1");
}

#[test]
fn export_format_serde_roundtrip() {
    for fmt in &[
        ExportFormat::Json,
        ExportFormat::Csv,
        ExportFormat::Structured,
    ] {
        let json = serde_json::to_string(fmt).unwrap();
        let fmt2: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(*fmt, fmt2);
    }
}

#[test]
fn event_with_rich_metadata_serde() {
    let mut meta = BTreeMap::new();
    meta.insert("string".to_string(), serde_json::json!("hello"));
    meta.insert("number".to_string(), serde_json::json!(42));
    meta.insert("boolean".to_string(), serde_json::json!(true));
    meta.insert("null".to_string(), serde_json::json!(null));
    meta.insert("array".to_string(), serde_json::json!([1, 2, 3]));
    let event = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::MappingPerformed,
        run_id: None,
        backend: None,
        metadata: meta,
        duration_ms: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.metadata["string"], "hello");
    assert_eq!(e2.metadata["number"], 42);
    assert_eq!(e2.metadata["boolean"], true);
    assert!(e2.metadata["null"].is_null());
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn empty_collector_summary_is_safe() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert!(s.average_run_duration_ms.is_none());
}

#[test]
fn filter_matches_nothing_collector_stays_empty() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::FallbackTriggered]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(f);
    c.record(ev(TelemetryEventType::RunStarted));
    c.record(ev(TelemetryEventType::RunCompleted));
    c.record(ev(TelemetryEventType::RunFailed));
    assert!(c.events().is_empty());
}

#[test]
fn large_event_count_collector() {
    let mut c = TelemetryCollector::new();
    for i in 0..1000 {
        c.record(make_event(
            TelemetryEventType::RunCompleted,
            "2025-01-01T00:00:00Z",
            Some(&format!("run-{i}")),
            None,
            Some(i as u64),
        ));
    }
    assert_eq!(c.events().len(), 1000);
    let s = c.summary();
    assert_eq!(s.total_events, 1000);
    assert_eq!(s.events_by_type["run_completed"], 1000);
    // Average of 0..999 = 499.5, integer division = 499.
    assert_eq!(s.average_run_duration_ms, Some(499));
}

#[test]
fn collector_with_filter_all_rejected_summary() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![]), // empty allowed list = nothing matches
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(f);
    c.record(ev(TelemetryEventType::RunStarted));
    let s = c.summary();
    assert_eq!(s.total_events, 0);
}

#[test]
fn clear_then_record_works() {
    let mut c = TelemetryCollector::new();
    c.record(ev(TelemetryEventType::RunStarted));
    c.clear();
    c.record(ev(TelemetryEventType::RunCompleted));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].event_type, TelemetryEventType::RunCompleted);
}

#[test]
fn event_with_none_optional_fields() {
    let e = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::RunStarted,
        run_id: None,
        backend: None,
        metadata: BTreeMap::new(),
        duration_ms: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert!(e2.run_id.is_none());
    assert!(e2.backend.is_none());
    assert!(e2.duration_ms.is_none());
    assert!(e2.metadata.is_empty());
}

// ===========================================================================
// 10. Metrics aggregation: count, sum, average, percentile
// ===========================================================================

#[test]
fn metrics_collector_count() {
    let c = MetricsCollector::new();
    assert_eq!(c.len(), 0);
    assert!(c.is_empty());
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("b", 200, 0));
    assert_eq!(c.len(), 2);
    assert!(!c.is_empty());
}

#[test]
fn metrics_collector_sum_tokens() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0)); // 100 in, 200 out
    c.record(simple_run("b", 200, 0)); // 100 in, 200 out
    c.record(simple_run("c", 300, 0)); // 100 in, 200 out
    let s = c.summary();
    assert_eq!(s.total_tokens_in, 300);
    assert_eq!(s.total_tokens_out, 600);
}

#[test]
fn metrics_collector_mean_duration() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 0));
    c.record(simple_run("a", 200, 0));
    c.record(simple_run("a", 300, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_collector_p50_odd() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40, 50] {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 30.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_collector_p50_even() {
    let c = MetricsCollector::new();
    for d in [10, 20, 30, 40] {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!((s.p50_duration_ms - 25.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_collector_p99() {
    let c = MetricsCollector::new();
    for d in 1..=100 {
        c.record(simple_run("a", d, 0));
    }
    let s = c.summary();
    assert!(s.p99_duration_ms > 98.0);
    assert!(s.p99_duration_ms <= 100.0);
}

#[test]
fn metrics_collector_error_rate() {
    let c = MetricsCollector::new();
    c.record(simple_run("a", 100, 1));
    c.record(simple_run("a", 200, 0));
    c.record(simple_run("a", 300, 2));
    let s = c.summary();
    // 3 errors / 3 runs = 1.0
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_collector_backend_counts() {
    let c = MetricsCollector::new();
    c.record(simple_run("alpha", 10, 0));
    c.record(simple_run("beta", 20, 0));
    c.record(simple_run("alpha", 30, 0));
    c.record(simple_run("gamma", 40, 0));
    let s = c.summary();
    assert_eq!(s.backend_counts["alpha"], 2);
    assert_eq!(s.backend_counts["beta"], 1);
    assert_eq!(s.backend_counts["gamma"], 1);
}

#[test]
fn empty_metrics_collector_summary() {
    let c = MetricsCollector::new();
    let s = c.summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert_eq!(s.total_tokens_in, 0);
    assert!(s.backend_counts.is_empty());
}

// ===========================================================================
// LatencyHistogram aggregation
// ===========================================================================

#[test]
fn histogram_empty() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.count(), 0);
    assert!(h.min().is_none());
    assert!(h.max().is_none());
    assert_eq!(h.mean(), 0.0);
    assert_eq!(h.p50(), 0.0);
    assert_eq!(h.p99(), 0.0);
}

#[test]
fn histogram_single_value() {
    let mut h = LatencyHistogram::new();
    h.record(42.0);
    assert_eq!(h.count(), 1);
    assert!(!h.is_empty());
    assert_eq!(h.min(), Some(42.0));
    assert_eq!(h.max(), Some(42.0));
    assert!((h.mean() - 42.0).abs() < f64::EPSILON);
    assert!((h.p50() - 42.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_multiple_values() {
    let mut h = LatencyHistogram::new();
    for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
        h.record(v);
    }
    assert_eq!(h.count(), 5);
    assert_eq!(h.min(), Some(10.0));
    assert_eq!(h.max(), Some(50.0));
    assert!((h.mean() - 30.0).abs() < f64::EPSILON);
    assert!((h.p50() - 30.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_percentile_interpolation() {
    let mut h = LatencyHistogram::new();
    // Values 1..=100
    for i in 1..=100 {
        h.record(i as f64);
    }
    let p95 = h.p95();
    assert!(p95 > 94.0);
    assert!(p95 <= 96.0);
    let p99 = h.p99();
    assert!(p99 > 98.0);
    assert!(p99 <= 100.0);
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
    assert!((h1.mean() - 25.0).abs() < f64::EPSILON);
}

#[test]
fn histogram_buckets() {
    let mut h = LatencyHistogram::new();
    for v in [5.0, 15.0, 25.0, 35.0, 45.0, 55.0] {
        h.record(v);
    }
    let boundaries = &[10.0, 20.0, 30.0, 40.0, 50.0];
    let counts = h.buckets(boundaries);
    // [0,10): 5.0 → 1
    // [10,20): 15.0 → 1
    // [20,30): 25.0 → 1
    // [30,40): 35.0 → 1
    // [40,50): 45.0 → 1
    // [50,∞): 55.0 → 1
    assert_eq!(counts, vec![1, 1, 1, 1, 1, 1]);
}

#[test]
fn histogram_serde_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
}

// ===========================================================================
// RunSummary aggregation
// ===========================================================================

#[test]
fn run_summary_from_events() {
    let s = RunSummary::from_events(&["error", "tool_call", "tool_call", "warning"], 500);
    assert_eq!(s.total_events, 4);
    assert_eq!(s.error_count, 1);
    assert_eq!(s.warning_count, 1);
    assert_eq!(s.tool_call_count, 2);
    assert_eq!(s.total_duration_ms, 500);
    assert!(s.has_errors());
}

#[test]
fn run_summary_error_rate() {
    let s = RunSummary::from_events(&["error", "ok", "ok", "ok"], 100);
    assert!((s.error_rate() - 0.25).abs() < f64::EPSILON);
}

#[test]
fn run_summary_error_rate_zero_events() {
    let s = RunSummary::new();
    assert!((s.error_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn run_summary_merge() {
    let s1 = RunSummary::from_events(&["error", "tool_call"], 100);
    let s2 = RunSummary::from_events(&["warning", "tool_call", "tool_call"], 200);
    let mut merged = RunSummary::new();
    merged.merge(&s1);
    merged.merge(&s2);
    assert_eq!(merged.total_events, 5);
    assert_eq!(merged.error_count, 1);
    assert_eq!(merged.warning_count, 1);
    assert_eq!(merged.tool_call_count, 3);
    assert_eq!(merged.total_duration_ms, 300);
}

#[test]
fn run_summary_no_errors() {
    let s = RunSummary::from_events(&["tool_call", "ok"], 100);
    assert!(!s.has_errors());
    assert!((s.error_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["error", "tool_call"], 500);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

// ===========================================================================
// CostEstimator
// ===========================================================================

#[test]
fn cost_estimator_no_pricing() {
    let est = CostEstimator::new();
    assert!(est.estimate("gpt-4", 100, 50).is_none());
    assert!(est.models().is_empty());
}

#[test]
fn cost_estimator_single_model() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.03,
            output_cost_per_token: 0.06,
        },
    );
    let cost = est.estimate("gpt-4", 1000, 500).unwrap();
    // 1000*0.03 + 500*0.06 = 30 + 30 = 60
    assert!((cost - 60.0).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_unknown_model_returns_none() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.03,
            output_cost_per_token: 0.06,
        },
    );
    assert!(est.estimate("claude-3", 100, 50).is_none());
}

#[test]
fn cost_estimator_total_across_models() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    est.set_pricing(
        "claude",
        ModelPricing {
            input_cost_per_token: 0.005,
            output_cost_per_token: 0.015,
        },
    );
    let total = est.estimate_total(&[("gpt-4", 100, 100), ("claude", 200, 200)]);
    // gpt-4: 100*0.01 + 100*0.02 = 3.0
    // claude: 200*0.005 + 200*0.015 = 4.0
    assert!((total - 7.0).abs() < f64::EPSILON);
}

#[test]
fn cost_estimator_total_skips_unknown() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.01,
            output_cost_per_token: 0.02,
        },
    );
    let total = est.estimate_total(&[("gpt-4", 100, 100), ("unknown", 100, 100)]);
    // Only gpt-4 counted: 100*0.01 + 100*0.02 = 3.0
    assert!((total - 3.0).abs() < f64::EPSILON);
}

// ===========================================================================
// MetricsExporter
// ===========================================================================

#[test]
fn exporter_json_valid() {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 0));
    let s = c.summary();
    let json = MetricsExporter::export_json(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn exporter_csv_has_header_and_data() {
    let runs = vec![simple_run("mock", 100, 0)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    assert!(csv.starts_with("backend_name,"));
    assert!(csv.contains("mock"));
}

#[test]
fn exporter_structured_format() {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 0));
    let s = c.summary();
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count=1"));
    assert!(out.contains("mean_duration_ms="));
    assert!(out.contains("backend.mock=1"));
}

#[test]
fn exporter_dispatch_by_format() {
    let c = MetricsCollector::new();
    c.record(simple_run("mock", 100, 0));
    let s = c.summary();
    // All three formats should succeed.
    assert!(MetricsExporter::export(&s, ExportFormat::Json).is_ok());
    assert!(MetricsExporter::export(&s, ExportFormat::Csv).is_ok());
    assert!(MetricsExporter::export(&s, ExportFormat::Structured).is_ok());
}

// ===========================================================================
// TelemetrySpan
// ===========================================================================

#[test]
fn span_new_has_no_attributes() {
    let span = TelemetrySpan::new("test-op");
    assert_eq!(span.name, "test-op");
    assert!(span.attributes.is_empty());
}

#[test]
fn span_with_attribute_chaining() {
    let span = TelemetrySpan::new("op")
        .with_attribute("k1", "v1")
        .with_attribute("k2", "v2")
        .with_attribute("k3", "v3");
    assert_eq!(span.attributes.len(), 3);
    assert_eq!(span.attributes["k1"], "v1");
    assert_eq!(span.attributes["k2"], "v2");
    assert_eq!(span.attributes["k3"], "v3");
}

// ===========================================================================
// Clone independence
// ===========================================================================

#[test]
fn telemetry_event_clone_independence() {
    let e1 = ev_run(TelemetryEventType::RunStarted, "r1");
    let mut e2 = e1.clone();
    e2.run_id = Some("r2".to_string());
    assert_eq!(e1.run_id.as_deref(), Some("r1"));
    assert_eq!(e2.run_id.as_deref(), Some("r2"));
}

#[test]
fn collector_clone_independence() {
    let mut c1 = TelemetryCollector::new();
    c1.record(ev(TelemetryEventType::RunStarted));
    let mut c2 = c1.clone();
    c2.record(ev(TelemetryEventType::RunFailed));
    assert_eq!(c1.events().len(), 1);
    assert_eq!(c2.events().len(), 2);
}
