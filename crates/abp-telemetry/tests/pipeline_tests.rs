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
//! Tests for the telemetry event pipeline.

use abp_telemetry::pipeline::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event(
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

fn event_with_meta(
    event_type: TelemetryEventType,
    key: &str,
    value: serde_json::Value,
) -> TelemetryEvent {
    let mut metadata = BTreeMap::new();
    metadata.insert(key.to_string(), value);
    TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type,
        run_id: None,
        backend: None,
        metadata,
        duration_ms: None,
    }
}

// ===========================================================================
// TelemetryEvent
// ===========================================================================

#[test]
fn event_serde_roundtrip() {
    let e = event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        Some("mock"),
        None,
    );
    let json = serde_json::to_string(&e).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.event_type, TelemetryEventType::RunStarted);
    assert_eq!(e2.run_id.as_deref(), Some("r1"));
    assert_eq!(e2.backend.as_deref(), Some("mock"));
}

#[test]
fn event_metadata_deterministic_order() {
    let mut meta = BTreeMap::new();
    meta.insert("z".to_string(), serde_json::json!("last"));
    meta.insert("a".to_string(), serde_json::json!("first"));
    let e = TelemetryEvent {
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        event_type: TelemetryEventType::MappingPerformed,
        run_id: None,
        backend: None,
        metadata: meta,
        duration_ms: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should serialize keys in order");
}

#[test]
fn event_type_display() {
    assert_eq!(TelemetryEventType::RunStarted.to_string(), "run_started");
    assert_eq!(
        TelemetryEventType::FallbackTriggered.to_string(),
        "fallback_triggered"
    );
}

#[test]
fn event_clone_independence() {
    let e1 = event(TelemetryEventType::RunStarted, Some("r1"), None, None);
    let mut e2 = e1.clone();
    e2.run_id = Some("r2".to_string());
    assert_eq!(e1.run_id.as_deref(), Some("r1"));
    assert_eq!(e2.run_id.as_deref(), Some("r2"));
}

// ===========================================================================
// TelemetryEventType
// ===========================================================================

#[test]
fn event_type_serde_roundtrip_all_variants() {
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
fn event_type_equality() {
    assert_eq!(
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunStarted
    );
    assert_ne!(
        TelemetryEventType::RunStarted,
        TelemetryEventType::RunFailed
    );
}

// ===========================================================================
// TelemetryFilter
// ===========================================================================

#[test]
fn filter_default_matches_all() {
    let f = TelemetryFilter::default();
    let e = event(TelemetryEventType::RunStarted, None, None, None);
    assert!(f.matches(&e));
}

#[test]
fn filter_allowed_types_accepts() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    assert!(f.matches(&event(TelemetryEventType::RunStarted, None, None, None)));
}

#[test]
fn filter_allowed_types_rejects() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: None,
    };
    assert!(!f.matches(&event(TelemetryEventType::RunFailed, None, None, None)));
}

#[test]
fn filter_min_duration_accepts() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    assert!(f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(150)
    )));
}

#[test]
fn filter_min_duration_rejects() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    assert!(!f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(50)
    )));
}

#[test]
fn filter_min_duration_passes_none_duration() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    // Events without a duration are not filtered by the duration predicate.
    assert!(f.matches(&event(TelemetryEventType::RunStarted, None, None, None)));
}

#[test]
fn filter_combined_predicates() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(50),
    };
    // Type matches, duration matches → pass.
    assert!(f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100)
    )));
    // Type matches, duration too low → reject.
    assert!(!f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(10)
    )));
    // Type does not match → reject.
    assert!(!f.matches(&event(TelemetryEventType::RunFailed, None, None, Some(100))));
}

#[test]
fn filter_serde_roundtrip() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunStarted]),
        min_duration_ms: Some(42),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: TelemetryFilter = serde_json::from_str(&json).unwrap();
    assert_eq!(f2.min_duration_ms, Some(42));
}

// ===========================================================================
// TelemetryCollector — basics
// ===========================================================================

#[test]
fn collector_new_is_empty() {
    let c = TelemetryCollector::new();
    assert!(c.events().is_empty());
}

#[test]
fn collector_record_and_retrieve() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].run_id.as_deref(), Some("r1"));
}

#[test]
fn collector_clear() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.clear();
    assert!(c.events().is_empty());
}

// ===========================================================================
// TelemetryCollector — querying
// ===========================================================================

#[test]
fn collector_events_of_type() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.record(event(TelemetryEventType::RunFailed, None, None, None));
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    let started = c.events_of_type(TelemetryEventType::RunStarted);
    assert_eq!(started.len(), 2);
}

#[test]
fn collector_events_of_type_empty() {
    let c = TelemetryCollector::new();
    assert!(
        c.events_of_type(TelemetryEventType::RetryAttempted)
            .is_empty()
    );
}

#[test]
fn collector_run_events() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    c.record(event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(100),
    ));
    c.record(event(
        TelemetryEventType::RunStarted,
        Some("r2"),
        None,
        None,
    ));
    let r1 = c.run_events("r1");
    assert_eq!(r1.len(), 2);
    assert!(r1.iter().all(|e| e.run_id.as_deref() == Some("r1")));
}

#[test]
fn collector_run_events_no_match() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    assert!(c.run_events("no-such-run").is_empty());
}

// ===========================================================================
// TelemetryCollector — with_filter
// ===========================================================================

#[test]
fn collector_with_filter_drops_non_matching() {
    let f = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: None,
    };
    let mut c = TelemetryCollector::with_filter(f);
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(50),
    ));
    assert_eq!(c.events().len(), 1);
    assert_eq!(c.events()[0].event_type, TelemetryEventType::RunCompleted);
}

#[test]
fn collector_without_filter_keeps_all() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.record(event(TelemetryEventType::RunFailed, None, None, None));
    assert_eq!(c.events().len(), 2);
}

// ===========================================================================
// TelemetrySummary
// ===========================================================================

#[test]
fn summary_empty_collector() {
    let c = TelemetryCollector::new();
    let s = c.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert_eq!(s.average_run_duration_ms, None);
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn summary_counts_by_type() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.record(event(TelemetryEventType::RunStarted, None, None, None));
    c.record(event(TelemetryEventType::BackendSelected, None, None, None));
    let s = c.summary();
    assert_eq!(s.total_events, 3);
    assert_eq!(s.events_by_type["run_started"], 2);
    assert_eq!(s.events_by_type["backend_selected"], 1);
}

#[test]
fn summary_average_run_duration() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(200),
    ));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(150));
}

#[test]
fn summary_average_duration_ignores_non_completed() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunStarted, None, None, Some(999)));
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    let s = c.summary();
    assert_eq!(s.average_run_duration_ms, Some(100));
}

#[test]
fn summary_error_rate() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(50),
    ));
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(60),
    ));
    c.record(event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    // 1 failed / (2 completed + 1 failed) = 1/3
    assert!((s.error_rate - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn summary_error_rate_all_failed() {
    let mut c = TelemetryCollector::new();
    c.record(event(TelemetryEventType::RunFailed, None, None, None));
    c.record(event(TelemetryEventType::RunFailed, None, None, None));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < 1e-9);
}

#[test]
fn summary_serde_roundtrip() {
    let mut c = TelemetryCollector::new();
    c.record(event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: TelemetrySummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s2.total_events, 1);
}

#[test]
fn event_with_metadata_roundtrip() {
    let e = event_with_meta(
        TelemetryEventType::BackendSelected,
        "reason",
        serde_json::json!("preferred"),
    );
    let json = serde_json::to_string(&e).unwrap();
    let e2: TelemetryEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e2.metadata["reason"], "preferred");
}

#[test]
fn filter_min_duration_exact_boundary() {
    let f = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(100),
    };
    // Exactly at boundary should pass.
    assert!(f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100)
    )));
    // One below should fail.
    assert!(!f.matches(&event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(99)
    )));
}
