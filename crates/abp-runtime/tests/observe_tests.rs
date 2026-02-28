// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the observe module.

use abp_runtime::observe::{
    ObservabilitySummary, RuntimeObserver, SpanStatus, TraceCollector,
};

// ---------------------------------------------------------------------------
// TraceCollector basics
// ---------------------------------------------------------------------------

#[test]
fn new_collector_has_no_spans() {
    let tc = TraceCollector::new();
    assert!(tc.spans().is_empty());
}

#[test]
fn start_span_returns_unique_ids() {
    let mut tc = TraceCollector::new();
    let a = tc.start_span("a");
    let b = tc.start_span("b");
    assert_ne!(a, b);
}

#[test]
fn start_span_records_name() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("my-op");
    assert_eq!(tc.spans()[0].name, "my-op");
    assert_eq!(tc.spans()[0].id, id);
}

#[test]
fn started_span_is_active() {
    let mut tc = TraceCollector::new();
    tc.start_span("op");
    assert_eq!(tc.active_spans().len(), 1);
}

#[test]
fn end_span_marks_inactive() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("op");
    tc.end_span(&id);
    assert!(tc.active_spans().is_empty());
    assert!(tc.spans()[0].end_time.is_some());
}

#[test]
fn end_span_noop_for_unknown_id() {
    let mut tc = TraceCollector::new();
    tc.end_span("nonexistent"); // should not panic
    assert!(tc.spans().is_empty());
}

#[test]
fn child_span_references_parent() {
    let mut tc = TraceCollector::new();
    let parent = tc.start_span("parent");
    let child = tc.start_child_span("child", &parent);
    let child_span = tc.spans().iter().find(|s| s.id == child).unwrap();
    assert_eq!(child_span.parent_id.as_deref(), Some(parent.as_str()));
}

#[test]
fn root_spans_excludes_children() {
    let mut tc = TraceCollector::new();
    let root = tc.start_span("root");
    tc.start_child_span("child", &root);
    let roots = tc.root_spans();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].id, root);
}

#[test]
fn children_returns_direct_children() {
    let mut tc = TraceCollector::new();
    let root = tc.start_span("root");
    let c1 = tc.start_child_span("c1", &root);
    let c2 = tc.start_child_span("c2", &root);
    tc.start_child_span("gc", &c1); // grandchild
    let children = tc.children(&root);
    assert_eq!(children.len(), 2);
    let ids: Vec<&str> = children.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&c1.as_str()));
    assert!(ids.contains(&c2.as_str()));
}

#[test]
fn set_status_updates_span() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("op");
    tc.set_status(&id, SpanStatus::Ok);
    assert_eq!(tc.spans()[0].status, SpanStatus::Ok);
}

#[test]
fn set_status_error() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("op");
    tc.set_status(
        &id,
        SpanStatus::Error {
            message: "boom".into(),
        },
    );
    assert!(matches!(
        tc.spans()[0].status,
        SpanStatus::Error { ref message } if message == "boom"
    ));
}

#[test]
fn set_attribute_attaches_kv() {
    let mut tc = TraceCollector::new();
    let id = tc.start_span("op");
    tc.set_attribute(&id, "key", "val");
    assert_eq!(tc.spans()[0].attributes.get("key").unwrap(), "val");
}

#[test]
fn to_json_produces_valid_json() {
    let mut tc = TraceCollector::new();
    tc.start_span("op");
    let json = tc.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 1);
}

#[test]
fn default_status_is_unset() {
    let mut tc = TraceCollector::new();
    tc.start_span("op");
    assert_eq!(tc.spans()[0].status, SpanStatus::Unset);
}

// ---------------------------------------------------------------------------
// RuntimeObserver
// ---------------------------------------------------------------------------

#[test]
fn observer_new_is_empty() {
    let obs = RuntimeObserver::new();
    assert!(obs.metrics().is_empty());
    let s = obs.summary();
    assert_eq!(
        s,
        ObservabilitySummary {
            total_spans: 0,
            active_spans: 0,
            error_spans: 0,
            metrics_count: 0,
        }
    );
}

#[test]
fn record_metric_stores_value() {
    let mut obs = RuntimeObserver::new();
    obs.record_metric("latency_ms", 42.5);
    assert_eq!(obs.metrics()["latency_ms"], 42.5);
}

#[test]
fn record_metric_overwrites() {
    let mut obs = RuntimeObserver::new();
    obs.record_metric("x", 1.0);
    obs.record_metric("x", 2.0);
    assert_eq!(obs.metrics()["x"], 2.0);
    assert_eq!(obs.metrics().len(), 1);
}

#[test]
fn trace_collector_via_observer() {
    let mut obs = RuntimeObserver::new();
    let id = obs.trace_collector().start_span("op");
    obs.trace_collector().end_span(&id);
    assert_eq!(obs.summary().total_spans, 1);
    assert_eq!(obs.summary().active_spans, 0);
}

#[test]
fn summary_counts_errors() {
    let mut obs = RuntimeObserver::new();
    let a = obs.trace_collector().start_span("ok-span");
    obs.trace_collector().set_status(&a, SpanStatus::Ok);
    let b = obs.trace_collector().start_span("err-span");
    obs.trace_collector().set_status(
        &b,
        SpanStatus::Error {
            message: "fail".into(),
        },
    );
    obs.record_metric("m", 1.0);
    let s = obs.summary();
    assert_eq!(s.total_spans, 2);
    assert_eq!(s.error_spans, 1);
    assert_eq!(s.metrics_count, 1);
}

#[test]
fn summary_tracks_active_spans() {
    let mut obs = RuntimeObserver::new();
    let _a = obs.trace_collector().start_span("active");
    let b = obs.trace_collector().start_span("done");
    obs.trace_collector().end_span(&b);
    let s = obs.summary();
    assert_eq!(s.active_spans, 1);
    assert_eq!(s.total_spans, 2);
}
