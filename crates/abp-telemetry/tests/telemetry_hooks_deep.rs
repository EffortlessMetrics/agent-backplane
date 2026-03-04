// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for telemetry hooks and event pipeline covering hook registration,
//! pre/post-execution hooks, event hooks, error hooks, metric collection, hook
//! ordering, failure handling, async hooks, hook filtering, span correlation,
//! metric aggregation, serde for metric types, and edge cases.
#![allow(clippy::float_cmp)]

use abp_telemetry::hooks::{
    ErrorClassification, RequestOutcome, on_error, on_request_complete, on_request_start,
};
use abp_telemetry::metrics::{
    ActiveRequestGauge, ErrorCounter, RequestCounter, RequestKey, TokenAccumulator,
};
use abp_telemetry::pipeline::*;
use abp_telemetry::spans;
use abp_telemetry::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mk_run(backend: &str, dur: u64, tokens_in: u64, tokens_out: u64, errors: u64) -> RunMetrics {
    RunMetrics {
        backend_name: backend.into(),
        dialect: "test".into(),
        duration_ms: dur,
        events_count: 3,
        tokens_in,
        tokens_out,
        tool_calls_count: 1,
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

/// Simulated hook registry using closures stored in an Arc<Mutex<Vec<…>>>.
struct HookRegistry {
    pre_hooks: Vec<Box<dyn Fn(&str, &str) + Send>>,
    post_hooks: Vec<Box<dyn Fn(&str, &RequestOutcome, u64) + Send>>,
    event_hooks: Vec<Box<dyn Fn(&str) + Send>>,
    error_hooks: Vec<Box<dyn Fn(&str, &str, ErrorClassification) + Send>>,
}

impl HookRegistry {
    fn new() -> Self {
        Self {
            pre_hooks: Vec::new(),
            post_hooks: Vec::new(),
            event_hooks: Vec::new(),
            error_hooks: Vec::new(),
        }
    }

    fn add_pre_hook(&mut self, f: impl Fn(&str, &str) + Send + 'static) {
        self.pre_hooks.push(Box::new(f));
    }

    fn add_post_hook(&mut self, f: impl Fn(&str, &RequestOutcome, u64) + Send + 'static) {
        self.post_hooks.push(Box::new(f));
    }

    fn add_event_hook(&mut self, f: impl Fn(&str) + Send + 'static) {
        self.event_hooks.push(Box::new(f));
    }

    fn add_error_hook(&mut self, f: impl Fn(&str, &str, ErrorClassification) + Send + 'static) {
        self.error_hooks.push(Box::new(f));
    }

    fn fire_pre(&self, work_order_id: &str, backend: &str) {
        for h in &self.pre_hooks {
            h(work_order_id, backend);
        }
    }

    fn fire_post(&self, work_order_id: &str, outcome: &RequestOutcome, elapsed: u64) {
        for h in &self.post_hooks {
            h(work_order_id, outcome, elapsed);
        }
    }

    fn fire_event(&self, kind: &str) {
        for h in &self.event_hooks {
            h(kind);
        }
    }

    fn fire_error(&self, code: &str, message: &str, classification: ErrorClassification) {
        for h in &self.error_hooks {
            h(code, message, classification);
        }
    }

    fn pre_hook_count(&self) -> usize {
        self.pre_hooks.len()
    }

    fn post_hook_count(&self) -> usize {
        self.post_hooks.len()
    }

    fn event_hook_count(&self) -> usize {
        self.event_hooks.len()
    }

    fn error_hook_count(&self) -> usize {
        self.error_hooks.len()
    }

    fn total_hooks(&self) -> usize {
        self.pre_hook_count()
            + self.post_hook_count()
            + self.event_hook_count()
            + self.error_hook_count()
    }

    fn remove_all_pre_hooks(&mut self) {
        self.pre_hooks.clear();
    }

    fn remove_all_post_hooks(&mut self) {
        self.post_hooks.clear();
    }

    fn remove_all_event_hooks(&mut self) {
        self.event_hooks.clear();
    }

    fn remove_all_error_hooks(&mut self) {
        self.error_hooks.clear();
    }
}

// =========================================================================
// 1. Hook registration (add/remove/list)
// =========================================================================

#[test]
fn registry_starts_empty() {
    let reg = HookRegistry::new();
    assert_eq!(reg.total_hooks(), 0);
}

#[test]
fn add_single_pre_hook() {
    let mut reg = HookRegistry::new();
    reg.add_pre_hook(|_, _| {});
    assert_eq!(reg.pre_hook_count(), 1);
}

#[test]
fn add_multiple_pre_hooks() {
    let mut reg = HookRegistry::new();
    for _ in 0..5 {
        reg.add_pre_hook(|_, _| {});
    }
    assert_eq!(reg.pre_hook_count(), 5);
}

#[test]
fn add_hooks_of_each_type() {
    let mut reg = HookRegistry::new();
    reg.add_pre_hook(|_, _| {});
    reg.add_post_hook(|_, _, _| {});
    reg.add_event_hook(|_| {});
    reg.add_error_hook(|_, _, _| {});
    assert_eq!(reg.total_hooks(), 4);
}

#[test]
fn remove_pre_hooks() {
    let mut reg = HookRegistry::new();
    reg.add_pre_hook(|_, _| {});
    reg.add_pre_hook(|_, _| {});
    reg.remove_all_pre_hooks();
    assert_eq!(reg.pre_hook_count(), 0);
}

#[test]
fn remove_post_hooks() {
    let mut reg = HookRegistry::new();
    reg.add_post_hook(|_, _, _| {});
    reg.remove_all_post_hooks();
    assert_eq!(reg.post_hook_count(), 0);
}

#[test]
fn remove_event_hooks() {
    let mut reg = HookRegistry::new();
    reg.add_event_hook(|_| {});
    reg.remove_all_event_hooks();
    assert_eq!(reg.event_hook_count(), 0);
}

#[test]
fn remove_error_hooks() {
    let mut reg = HookRegistry::new();
    reg.add_error_hook(|_, _, _| {});
    reg.remove_all_error_hooks();
    assert_eq!(reg.error_hook_count(), 0);
}

#[test]
fn remove_does_not_affect_other_types() {
    let mut reg = HookRegistry::new();
    reg.add_pre_hook(|_, _| {});
    reg.add_post_hook(|_, _, _| {});
    reg.remove_all_pre_hooks();
    assert_eq!(reg.pre_hook_count(), 0);
    assert_eq!(reg.post_hook_count(), 1);
}

#[test]
fn list_hook_counts_after_mixed_operations() {
    let mut reg = HookRegistry::new();
    reg.add_pre_hook(|_, _| {});
    reg.add_pre_hook(|_, _| {});
    reg.add_post_hook(|_, _, _| {});
    reg.add_event_hook(|_| {});
    reg.add_event_hook(|_| {});
    reg.add_event_hook(|_| {});
    reg.add_error_hook(|_, _, _| {});
    assert_eq!(reg.pre_hook_count(), 2);
    assert_eq!(reg.post_hook_count(), 1);
    assert_eq!(reg.event_hook_count(), 3);
    assert_eq!(reg.error_hook_count(), 1);
    assert_eq!(reg.total_hooks(), 7);
}

// =========================================================================
// 2. Pre-execution hooks (called before backend run)
// =========================================================================

#[test]
fn pre_hook_receives_work_order_and_backend() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    let cap = captured.clone();
    reg.add_pre_hook(move |wo, be| {
        cap.lock().unwrap().push((wo.to_string(), be.to_string()));
    });
    reg.fire_pre("wo-1", "mock");
    let data = captured.lock().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0], ("wo-1".into(), "mock".into()));
}

#[test]
fn pre_hook_integrates_with_on_request_start() {
    let started = Arc::new(Mutex::new(false));
    let mut reg = HookRegistry::new();
    let s = started.clone();
    reg.add_pre_hook(move |_, _| {
        *s.lock().unwrap() = true;
    });
    reg.fire_pre("wo-pre", "sidecar:node");
    let _instant = on_request_start("wo-pre", "sidecar:node");
    assert!(*started.lock().unwrap());
}

#[test]
fn multiple_pre_hooks_all_called() {
    let count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    for _ in 0..3 {
        let c = count.clone();
        reg.add_pre_hook(move |_, _| {
            *c.lock().unwrap() += 1;
        });
    }
    reg.fire_pre("wo-x", "mock");
    assert_eq!(*count.lock().unwrap(), 3);
}

#[test]
fn pre_hook_sees_correct_backend_name() {
    let seen = Arc::new(Mutex::new(String::new()));
    let mut reg = HookRegistry::new();
    let s = seen.clone();
    reg.add_pre_hook(move |_, be| {
        *s.lock().unwrap() = be.to_string();
    });
    reg.fire_pre("wo-abc", "sidecar:claude");
    assert_eq!(*seen.lock().unwrap(), "sidecar:claude");
}

// =========================================================================
// 3. Post-execution hooks (called after backend run with receipt)
// =========================================================================

#[test]
fn post_hook_receives_outcome_and_elapsed() {
    let captured = Arc::new(Mutex::new(None));
    let mut reg = HookRegistry::new();
    let cap = captured.clone();
    reg.add_post_hook(move |wo, outcome, elapsed| {
        *cap.lock().unwrap() = Some((wo.to_string(), outcome.clone(), elapsed));
    });
    let outcome = RequestOutcome::Success;
    reg.fire_post("wo-done", &outcome, 42);
    let data = captured.lock().unwrap();
    let (wo, out, el) = data.as_ref().unwrap();
    assert_eq!(wo, "wo-done");
    assert_eq!(*out, RequestOutcome::Success);
    assert_eq!(*el, 42);
}

#[test]
fn post_hook_with_error_outcome() {
    let captured = Arc::new(Mutex::new(None));
    let mut reg = HookRegistry::new();
    let cap = captured.clone();
    reg.add_post_hook(move |_, outcome, _| {
        *cap.lock().unwrap() = Some(outcome.clone());
    });
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    reg.fire_post("wo-err", &outcome, 100);
    let data = captured.lock().unwrap();
    match data.as_ref().unwrap() {
        RequestOutcome::Error { code, message } => {
            assert_eq!(code, "timeout");
            assert_eq!(message, "deadline exceeded");
        }
        _ => panic!("expected error outcome"),
    }
}

#[test]
fn post_hook_integrates_with_on_request_complete() {
    let start = on_request_start("wo-post", "mock");
    let elapsed = on_request_complete("wo-post", "mock", &RequestOutcome::Success, start);

    let captured_elapsed = Arc::new(Mutex::new(0u64));
    let mut reg = HookRegistry::new();
    let cap = captured_elapsed.clone();
    reg.add_post_hook(move |_, _, el| {
        *cap.lock().unwrap() = el;
    });
    reg.fire_post("wo-post", &RequestOutcome::Success, elapsed);
    assert_eq!(*captured_elapsed.lock().unwrap(), elapsed);
}

#[test]
fn multiple_post_hooks_all_called() {
    let count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    for _ in 0..4 {
        let c = count.clone();
        reg.add_post_hook(move |_, _, _| {
            *c.lock().unwrap() += 1;
        });
    }
    reg.fire_post("wo-multi", &RequestOutcome::Success, 10);
    assert_eq!(*count.lock().unwrap(), 4);
}

// =========================================================================
// 4. Event hooks (called for each AgentEvent)
// =========================================================================

#[test]
fn event_hook_receives_kind() {
    let kinds = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    let k = kinds.clone();
    reg.add_event_hook(move |kind| {
        k.lock().unwrap().push(kind.to_string());
    });
    reg.fire_event("tool_call");
    reg.fire_event("error");
    reg.fire_event("message");
    let data = kinds.lock().unwrap();
    assert_eq!(data.len(), 3);
    assert_eq!(data[0], "tool_call");
    assert_eq!(data[1], "error");
    assert_eq!(data[2], "message");
}

#[test]
fn event_hook_called_per_event_in_run_summary() {
    let count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    let c = count.clone();
    reg.add_event_hook(move |_| {
        *c.lock().unwrap() += 1;
    });

    let mut summary = RunSummary::new();
    let events = ["tool_call", "error", "message", "tool_call"];
    for kind in &events {
        summary.record_event(kind);
        reg.fire_event(kind);
    }
    assert_eq!(*count.lock().unwrap(), 4);
    assert_eq!(summary.total_events, 4);
}

#[test]
fn event_hook_with_pipeline_event_types() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    let cap = captured.clone();
    reg.add_event_hook(move |kind| {
        cap.lock().unwrap().push(kind.to_string());
    });
    let types = [
        "run_started",
        "run_completed",
        "run_failed",
        "backend_selected",
    ];
    for t in &types {
        reg.fire_event(t);
    }
    let data = captured.lock().unwrap();
    assert_eq!(data.len(), 4);
}

#[test]
fn event_hook_records_to_run_summary() {
    let mut summary = RunSummary::new();
    let events = ["tool_call", "error", "warning", "tool_call", "message"];
    for kind in &events {
        summary.record_event(kind);
    }
    assert_eq!(summary.total_events, 5);
    assert_eq!(summary.tool_call_count, 2);
    assert_eq!(summary.error_count, 1);
    assert_eq!(summary.warning_count, 1);
    assert!(summary.has_errors());
}

// =========================================================================
// 5. Error hooks (called on failures with classification)
// =========================================================================

#[test]
fn error_hook_receives_code_message_classification() {
    let captured = Arc::new(Mutex::new(None));
    let mut reg = HookRegistry::new();
    let cap = captured.clone();
    reg.add_error_hook(move |code, msg, class| {
        *cap.lock().unwrap() = Some((code.to_string(), msg.to_string(), class));
    });
    reg.fire_error("E001", "transient failure", ErrorClassification::Transient);
    let data = captured.lock().unwrap();
    let (code, msg, class) = data.as_ref().unwrap();
    assert_eq!(code, "E001");
    assert_eq!(msg, "transient failure");
    assert_eq!(*class, ErrorClassification::Transient);
}

#[test]
fn error_hook_permanent_classification() {
    let cls = Arc::new(Mutex::new(None));
    let mut reg = HookRegistry::new();
    let c = cls.clone();
    reg.add_error_hook(move |_, _, class| {
        *c.lock().unwrap() = Some(class);
    });
    reg.fire_error("E002", "auth failed", ErrorClassification::Permanent);
    assert_eq!(*cls.lock().unwrap(), Some(ErrorClassification::Permanent));
}

#[test]
fn error_hook_unknown_classification() {
    let cls = Arc::new(Mutex::new(None));
    let mut reg = HookRegistry::new();
    let c = cls.clone();
    reg.add_error_hook(move |_, _, class| {
        *c.lock().unwrap() = Some(class);
    });
    reg.fire_error("E999", "mystery", ErrorClassification::Unknown);
    assert_eq!(*cls.lock().unwrap(), Some(ErrorClassification::Unknown));
}

#[test]
fn error_hook_integrates_with_on_error() {
    let called = Arc::new(Mutex::new(false));
    let mut reg = HookRegistry::new();
    let c = called.clone();
    reg.add_error_hook(move |_, _, _| {
        *c.lock().unwrap() = true;
    });
    on_error("wo-x", "E001", "oops", ErrorClassification::Transient);
    reg.fire_error("E001", "oops", ErrorClassification::Transient);
    assert!(*called.lock().unwrap());
}

#[test]
fn error_hook_with_error_counter() {
    let counter = ErrorCounter::new();
    let mut reg = HookRegistry::new();
    let ec = counter.clone();
    reg.add_error_hook(move |code, _, _| {
        ec.increment(code);
    });
    reg.fire_error("timeout", "took too long", ErrorClassification::Transient);
    reg.fire_error("timeout", "again", ErrorClassification::Transient);
    reg.fire_error("auth_fail", "bad creds", ErrorClassification::Permanent);
    assert_eq!(counter.get("timeout"), 2);
    assert_eq!(counter.get("auth_fail"), 1);
    assert_eq!(counter.total(), 3);
}

#[test]
fn error_classification_display_values() {
    assert_eq!(ErrorClassification::Transient.to_string(), "transient");
    assert_eq!(ErrorClassification::Permanent.to_string(), "permanent");
    assert_eq!(ErrorClassification::Unknown.to_string(), "unknown");
}

// =========================================================================
// 6. Metric collection (latency, token counts, event counts)
// =========================================================================

#[test]
fn latency_histogram_records_values() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);
    assert_eq!(h.count(), 3);
    assert!(!h.is_empty());
    assert_eq!(h.min(), Some(10.0));
    assert_eq!(h.max(), Some(30.0));
}

#[test]
fn latency_histogram_mean() {
    let mut h = LatencyHistogram::new();
    h.record(10.0);
    h.record(20.0);
    h.record(30.0);
    assert!((h.mean() - 20.0).abs() < f64::EPSILON);
}

#[test]
fn latency_histogram_percentiles() {
    let mut h = LatencyHistogram::new();
    for i in 1..=100 {
        h.record(i as f64);
    }
    assert!(h.p50() > 49.0 && h.p50() < 51.0);
    assert!(h.p95() > 94.0 && h.p95() < 96.0);
    assert!(h.p99() > 98.0 && h.p99() <= 100.0);
}

#[test]
fn latency_histogram_buckets() {
    let mut h = LatencyHistogram::new();
    for i in 0..100 {
        h.record(i as f64);
    }
    let buckets = h.buckets(&[25.0, 50.0, 75.0]);
    assert_eq!(buckets.len(), 4);
    assert_eq!(buckets[0], 25); // [0, 25)
    assert_eq!(buckets[1], 25); // [25, 50)
    assert_eq!(buckets[2], 25); // [50, 75)
    assert_eq!(buckets[3], 25); // [75, ∞)
}

#[test]
fn token_accumulator_tracks_totals() {
    let tok = TokenAccumulator::new();
    tok.add(100, 200);
    tok.add(50, 75);
    assert_eq!(tok.total_input(), 150);
    assert_eq!(tok.total_output(), 275);
    assert_eq!(tok.total(), 425);
}

#[test]
fn token_accumulator_reset() {
    let tok = TokenAccumulator::new();
    tok.add(100, 200);
    tok.reset();
    assert_eq!(tok.total(), 0);
}

#[test]
fn metrics_collector_records_event_counts() {
    let c = MetricsCollector::new();
    c.record(RunMetrics {
        events_count: 10,
        ..RunMetrics::default()
    });
    c.record(RunMetrics {
        events_count: 20,
        ..RunMetrics::default()
    });
    let runs = c.runs();
    let total_events: u64 = runs.iter().map(|r| r.events_count).sum();
    assert_eq!(total_events, 30);
}

#[test]
fn metrics_collector_tracks_latency_via_duration() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 100, 0, 0, 0));
    c.record(mk_run("a", 200, 0, 0, 0));
    c.record(mk_run("a", 300, 0, 0, 0));
    let s = c.summary();
    assert!((s.mean_duration_ms - 200.0).abs() < f64::EPSILON);
}

#[test]
fn request_counter_increments() {
    let rc = RequestCounter::new();
    rc.increment("mock", "openai", "success");
    rc.increment("mock", "openai", "success");
    rc.increment("mock", "openai", "error");
    assert_eq!(rc.get("mock", "openai", "success"), 2);
    assert_eq!(rc.total(), 3);
}

#[test]
fn gauge_tracks_active_requests() {
    let g = ActiveRequestGauge::new();
    g.increment();
    g.increment();
    assert_eq!(g.get(), 2);
    g.decrement();
    assert_eq!(g.get(), 1);
}

// =========================================================================
// 7. Hook ordering (multiple hooks execute in registration order)
// =========================================================================

#[test]
fn pre_hooks_execute_in_registration_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    for i in 0..5 {
        let o = order.clone();
        reg.add_pre_hook(move |_, _| {
            o.lock().unwrap().push(i);
        });
    }
    reg.fire_pre("wo-order", "mock");
    let data = order.lock().unwrap();
    assert_eq!(*data, vec![0, 1, 2, 3, 4]);
}

#[test]
fn post_hooks_execute_in_registration_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    for i in 0..4 {
        let o = order.clone();
        reg.add_post_hook(move |_, _, _| {
            o.lock().unwrap().push(i);
        });
    }
    reg.fire_post("wo-order", &RequestOutcome::Success, 0);
    let data = order.lock().unwrap();
    assert_eq!(*data, vec![0, 1, 2, 3]);
}

#[test]
fn event_hooks_execute_in_registration_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    for i in 0..3 {
        let o = order.clone();
        reg.add_event_hook(move |_| {
            o.lock().unwrap().push(i);
        });
    }
    reg.fire_event("tool_call");
    let data = order.lock().unwrap();
    assert_eq!(*data, vec![0, 1, 2]);
}

#[test]
fn error_hooks_execute_in_registration_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    for i in 0..3 {
        let o = order.clone();
        reg.add_error_hook(move |_, _, _| {
            o.lock().unwrap().push(i);
        });
    }
    reg.fire_error("E001", "err", ErrorClassification::Unknown);
    let data = order.lock().unwrap();
    assert_eq!(*data, vec![0, 1, 2]);
}

#[test]
fn mixed_hook_types_maintain_independent_ordering() {
    let pre_order = Arc::new(Mutex::new(Vec::new()));
    let post_order = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();

    for i in 0..3 {
        let o = pre_order.clone();
        reg.add_pre_hook(move |_, _| {
            o.lock().unwrap().push(i);
        });
    }
    for i in 10..13 {
        let o = post_order.clone();
        reg.add_post_hook(move |_, _, _| {
            o.lock().unwrap().push(i);
        });
    }

    reg.fire_pre("wo-mix", "mock");
    reg.fire_post("wo-mix", &RequestOutcome::Success, 0);

    assert_eq!(*pre_order.lock().unwrap(), vec![0, 1, 2]);
    assert_eq!(*post_order.lock().unwrap(), vec![10, 11, 12]);
}

// =========================================================================
// 8. Hook failure handling (one hook failing doesn't block others)
// =========================================================================

/// A registry variant that catches panics in individual hooks.
struct SafeHookRegistry {
    pre_hooks: Vec<Box<dyn Fn(&str, &str) + Send>>,
    post_hooks: Vec<Box<dyn Fn(&str, &RequestOutcome, u64) + Send>>,
    event_hooks: Vec<Box<dyn Fn(&str) + Send>>,
    error_hooks: Vec<Box<dyn Fn(&str, &str, ErrorClassification) + Send>>,
}

impl SafeHookRegistry {
    fn new() -> Self {
        Self {
            pre_hooks: Vec::new(),
            post_hooks: Vec::new(),
            event_hooks: Vec::new(),
            error_hooks: Vec::new(),
        }
    }

    fn add_pre_hook(&mut self, f: impl Fn(&str, &str) + Send + 'static) {
        self.pre_hooks.push(Box::new(f));
    }

    fn add_event_hook(&mut self, f: impl Fn(&str) + Send + 'static) {
        self.event_hooks.push(Box::new(f));
    }

    fn add_post_hook(&mut self, f: impl Fn(&str, &RequestOutcome, u64) + Send + 'static) {
        self.post_hooks.push(Box::new(f));
    }

    fn add_error_hook(&mut self, f: impl Fn(&str, &str, ErrorClassification) + Send + 'static) {
        self.error_hooks.push(Box::new(f));
    }

    fn fire_pre_safe(&self, wo: &str, be: &str) -> usize {
        let mut succeeded = 0;
        for h in &self.pre_hooks {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(wo, be)));
            if result.is_ok() {
                succeeded += 1;
            }
        }
        succeeded
    }

    fn fire_event_safe(&self, kind: &str) -> usize {
        let mut succeeded = 0;
        for h in &self.event_hooks {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(kind)));
            if result.is_ok() {
                succeeded += 1;
            }
        }
        succeeded
    }

    fn fire_post_safe(&self, wo: &str, outcome: &RequestOutcome, elapsed: u64) -> usize {
        let mut succeeded = 0;
        for h in &self.post_hooks {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(wo, outcome, elapsed)));
            if result.is_ok() {
                succeeded += 1;
            }
        }
        succeeded
    }

    fn fire_error_safe(&self, code: &str, msg: &str, class: ErrorClassification) -> usize {
        let mut succeeded = 0;
        for h in &self.error_hooks {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(code, msg, class)));
            if result.is_ok() {
                succeeded += 1;
            }
        }
        succeeded
    }
}

#[test]
fn pre_hook_panic_does_not_block_others() {
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut reg = SafeHookRegistry::new();

    let c1 = called.clone();
    reg.add_pre_hook(move |_, _| {
        c1.lock().unwrap().push(1);
    });
    reg.add_pre_hook(|_, _| {
        panic!("intentional panic");
    });
    let c3 = called.clone();
    reg.add_pre_hook(move |_, _| {
        c3.lock().unwrap().push(3);
    });

    let ok = reg.fire_pre_safe("wo-panic", "mock");
    assert_eq!(ok, 2);
    let data = called.lock().unwrap();
    assert_eq!(*data, vec![1, 3]);
}

#[test]
fn event_hook_panic_does_not_block_others() {
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut reg = SafeHookRegistry::new();

    let c1 = called.clone();
    reg.add_event_hook(move |_| {
        c1.lock().unwrap().push("a");
    });
    reg.add_event_hook(|_| {
        panic!("event hook crash");
    });
    let c3 = called.clone();
    reg.add_event_hook(move |_| {
        c3.lock().unwrap().push("c");
    });

    let ok = reg.fire_event_safe("tool_call");
    assert_eq!(ok, 2);
    let data = called.lock().unwrap();
    assert_eq!(*data, vec!["a", "c"]);
}

#[test]
fn post_hook_panic_does_not_block_others() {
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut reg = SafeHookRegistry::new();

    let c1 = called.clone();
    reg.add_post_hook(move |_, _, _| {
        c1.lock().unwrap().push(1);
    });
    reg.add_post_hook(|_, _, _| {
        panic!("post hook crash");
    });
    let c3 = called.clone();
    reg.add_post_hook(move |_, _, _| {
        c3.lock().unwrap().push(3);
    });

    let ok = reg.fire_post_safe("wo-x", &RequestOutcome::Success, 0);
    assert_eq!(ok, 2);
    assert_eq!(*called.lock().unwrap(), vec![1, 3]);
}

#[test]
fn error_hook_panic_does_not_block_others() {
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut reg = SafeHookRegistry::new();

    let c1 = called.clone();
    reg.add_error_hook(move |_, _, _| {
        c1.lock().unwrap().push(1);
    });
    reg.add_error_hook(|_, _, _| {
        panic!("error hook crash");
    });
    let c3 = called.clone();
    reg.add_error_hook(move |_, _, _| {
        c3.lock().unwrap().push(3);
    });

    let ok = reg.fire_error_safe("E001", "err", ErrorClassification::Unknown);
    assert_eq!(ok, 2);
    assert_eq!(*called.lock().unwrap(), vec![1, 3]);
}

#[test]
fn all_hooks_panic_returns_zero_succeeded() {
    let mut reg = SafeHookRegistry::new();
    for _ in 0..3 {
        reg.add_pre_hook(|_, _| panic!("boom"));
    }
    let ok = reg.fire_pre_safe("wo-all-fail", "mock");
    assert_eq!(ok, 0);
}

// =========================================================================
// 9. Async hooks (hooks performing async-like work via threads)
// =========================================================================

#[test]
fn pre_hook_spawns_background_work() {
    let result = Arc::new(Mutex::new(Vec::new()));
    let mut reg = HookRegistry::new();
    let r = result.clone();
    reg.add_pre_hook(move |wo, _| {
        let rr = r.clone();
        let wo_s = wo.to_string();
        let handle = thread::spawn(move || {
            rr.lock().unwrap().push(wo_s);
        });
        handle.join().unwrap();
    });
    reg.fire_pre("wo-async", "mock");
    assert_eq!(result.lock().unwrap().len(), 1);
}

#[test]
fn post_hook_with_delayed_metric_recording() {
    let collector = MetricsCollector::new();
    let mut reg = HookRegistry::new();
    let c = collector.clone();
    reg.add_post_hook(move |_, _, elapsed| {
        c.record(RunMetrics {
            duration_ms: elapsed,
            backend_name: "async_mock".into(),
            ..RunMetrics::default()
        });
    });
    reg.fire_post("wo-async-post", &RequestOutcome::Success, 123);
    assert_eq!(collector.len(), 1);
    assert_eq!(collector.runs()[0].duration_ms, 123);
}

#[test]
fn concurrent_hook_fires_from_threads() {
    let count = Arc::new(Mutex::new(0u32));
    let reg = Arc::new(Mutex::new(HookRegistry::new()));
    {
        let c = count.clone();
        reg.lock().unwrap().add_pre_hook(move |_, _| {
            *c.lock().unwrap() += 1;
        });
    }
    let mut handles = vec![];
    for i in 0..10 {
        let r = reg.clone();
        handles.push(thread::spawn(move || {
            r.lock().unwrap().fire_pre(&format!("wo-{}", i), "mock");
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(*count.lock().unwrap(), 10);
}

#[test]
fn event_hook_collects_to_shared_summary() {
    let summary = Arc::new(Mutex::new(RunSummary::new()));
    let mut reg = HookRegistry::new();
    let s = summary.clone();
    reg.add_event_hook(move |kind| {
        s.lock().unwrap().record_event(kind);
    });
    for kind in &["tool_call", "error", "message", "tool_call", "warning"] {
        reg.fire_event(kind);
    }
    let s = summary.lock().unwrap();
    assert_eq!(s.total_events, 5);
    assert_eq!(s.tool_call_count, 2);
    assert_eq!(s.error_count, 1);
}

// =========================================================================
// 10. Hook filtering (hooks that conditionally fire based on event type)
// =========================================================================

#[test]
fn event_hook_filters_by_kind() {
    let tool_calls = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    let tc = tool_calls.clone();
    reg.add_event_hook(move |kind| {
        if kind == "tool_call" {
            *tc.lock().unwrap() += 1;
        }
    });
    reg.fire_event("tool_call");
    reg.fire_event("message");
    reg.fire_event("tool_call");
    reg.fire_event("error");
    assert_eq!(*tool_calls.lock().unwrap(), 2);
}

#[test]
fn error_hook_filters_by_classification() {
    let transient_count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    let tc = transient_count.clone();
    reg.add_error_hook(move |_, _, class| {
        if class == ErrorClassification::Transient {
            *tc.lock().unwrap() += 1;
        }
    });
    reg.fire_error("E001", "a", ErrorClassification::Transient);
    reg.fire_error("E002", "b", ErrorClassification::Permanent);
    reg.fire_error("E003", "c", ErrorClassification::Transient);
    assert_eq!(*transient_count.lock().unwrap(), 2);
}

#[test]
fn telemetry_filter_by_allowed_types() {
    let filter = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: None,
    };
    let mut collector = TelemetryCollector::with_filter(filter);
    collector.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    collector.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    collector.record(mk_event(TelemetryEventType::RunFailed, None, None, None));
    assert_eq!(collector.events().len(), 1);
    assert_eq!(
        collector.events()[0].event_type,
        TelemetryEventType::RunCompleted
    );
}

#[test]
fn telemetry_filter_by_min_duration() {
    let filter = TelemetryFilter {
        allowed_types: None,
        min_duration_ms: Some(50),
    };
    let mut collector = TelemetryCollector::with_filter(filter);
    collector.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(10),
    ));
    collector.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    collector.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    // duration 10 is below minimum → filtered out; None duration passes
    assert_eq!(collector.events().len(), 2);
}

#[test]
fn telemetry_filter_combined_type_and_duration() {
    let filter = TelemetryFilter {
        allowed_types: Some(vec![TelemetryEventType::RunCompleted]),
        min_duration_ms: Some(50),
    };
    let mut collector = TelemetryCollector::with_filter(filter);
    collector.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(100),
    ));
    collector.record(mk_event(
        TelemetryEventType::RunCompleted,
        None,
        None,
        Some(10),
    ));
    collector.record(mk_event(
        TelemetryEventType::RunStarted,
        None,
        None,
        Some(100),
    ));
    assert_eq!(collector.events().len(), 1);
}

#[test]
fn post_hook_filters_by_outcome() {
    let success_count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    let sc = success_count.clone();
    reg.add_post_hook(move |_, outcome, _| {
        if *outcome == RequestOutcome::Success {
            *sc.lock().unwrap() += 1;
        }
    });
    reg.fire_post("wo-1", &RequestOutcome::Success, 0);
    reg.fire_post(
        "wo-2",
        &RequestOutcome::Error {
            code: "x".into(),
            message: "y".into(),
        },
        0,
    );
    reg.fire_post("wo-3", &RequestOutcome::Success, 0);
    assert_eq!(*success_count.lock().unwrap(), 2);
}

// =========================================================================
// 11. Span creation and correlation (tracing spans for runs)
// =========================================================================

#[test]
fn request_span_carries_fields() {
    let span = spans::request_span("wo-corr-1", "correlate this", "mapped");
    let _g = span.enter();
    // If no panic, the span was created and entered successfully.
}

#[test]
fn event_span_correlates_with_parent_request() {
    let parent = spans::request_span("wo-corr-2", "parent task", "mapped");
    let _p = parent.enter();
    let child = spans::event_span("tool_call", 1);
    let _c = child.enter();
}

#[test]
fn backend_span_correlates_with_request() {
    let req = spans::request_span("wo-corr-3", "backend work", "passthrough");
    let _r = req.enter();
    let be = spans::backend_span("sidecar:node");
    let _b = be.enter();
}

#[test]
fn telemetry_span_with_run_correlation() {
    let span = TelemetrySpan::new("run")
        .with_attribute("run_id", "wo-corr-4")
        .with_attribute("backend", "mock")
        .with_attribute("lane", "mapped");
    assert_eq!(span.attributes["run_id"], "wo-corr-4");
    assert_eq!(span.attributes["backend"], "mock");
    assert_eq!(span.attributes["lane"], "mapped");
}

#[test]
fn nested_spans_for_full_run_lifecycle() {
    let req = spans::request_span("wo-lifecycle", "deep test", "mapped");
    let _r = req.enter();
    let be = spans::backend_span("mock");
    let _b = be.enter();
    for seq in 0..5 {
        let ev = spans::event_span("step", seq);
        let _e = ev.enter();
    }
}

#[test]
fn span_emit_does_not_panic() {
    let span = TelemetrySpan::new("emit_test").with_attribute("key", "value");
    span.emit();
}

// =========================================================================
// 12. Metric aggregation (across multiple runs)
// =========================================================================

#[test]
fn collector_aggregates_multiple_backends() {
    let c = MetricsCollector::new();
    c.record(mk_run("openai", 100, 50, 100, 0));
    c.record(mk_run("anthropic", 200, 75, 150, 1));
    c.record(mk_run("openai", 300, 60, 120, 0));
    let s = c.summary();
    assert_eq!(s.count, 3);
    assert_eq!(s.backend_counts["openai"], 2);
    assert_eq!(s.backend_counts["anthropic"], 1);
    assert_eq!(s.total_tokens_in, 185);
    assert_eq!(s.total_tokens_out, 370);
}

#[test]
fn collector_summary_error_rate() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 10, 0, 0, 1));
    c.record(mk_run("a", 20, 0, 0, 0));
    c.record(mk_run("a", 30, 0, 0, 2));
    let s = c.summary();
    assert!((s.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn run_summary_merge() {
    let s1 = RunSummary::from_events(&["tool_call", "error"], 100);
    let s2 = RunSummary::from_events(&["message", "tool_call"], 200);
    let mut merged = s1.clone();
    merged.merge(&s2);
    assert_eq!(merged.total_events, 4);
    assert_eq!(merged.tool_call_count, 2);
    assert_eq!(merged.error_count, 1);
    assert_eq!(merged.total_duration_ms, 300);
}

#[test]
fn run_summary_error_rate_calculation() {
    let s = RunSummary::from_events(&["tool_call", "error", "error", "message"], 0);
    assert!((s.error_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn latency_histogram_merge() {
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
fn collector_clear_and_rerecord() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 100, 50, 100, 0));
    c.clear();
    assert!(c.is_empty());
    c.record(mk_run("b", 200, 75, 150, 0));
    assert_eq!(c.len(), 1);
    assert_eq!(c.runs()[0].backend_name, "b");
}

#[test]
fn telemetry_collector_summary_across_events() {
    let mut tc = TelemetryCollector::new();
    tc.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        Some("mock"),
        None,
    ));
    tc.record(mk_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        Some("mock"),
        Some(100),
    ));
    tc.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("r2"),
        Some("mock"),
        None,
    ));
    tc.record(mk_event(
        TelemetryEventType::RunFailed,
        Some("r2"),
        Some("mock"),
        None,
    ));
    let s = tc.summary();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.average_run_duration_ms, Some(100));
    assert!((s.error_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn telemetry_collector_events_by_run() {
    let mut tc = TelemetryCollector::new();
    tc.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("r1"),
        None,
        None,
    ));
    tc.record(mk_event(
        TelemetryEventType::RunCompleted,
        Some("r1"),
        None,
        Some(50),
    ));
    tc.record(mk_event(
        TelemetryEventType::RunStarted,
        Some("r2"),
        None,
        None,
    ));
    let r1_events = tc.run_events("r1");
    assert_eq!(r1_events.len(), 2);
    let r2_events = tc.run_events("r2");
    assert_eq!(r2_events.len(), 1);
}

// =========================================================================
// 13. Serde for metric types
// =========================================================================

#[test]
fn run_metrics_serde_roundtrip() {
    let m = mk_run("serde_test", 500, 100, 200, 1);
    let json = serde_json::to_string(&m).unwrap();
    let m2: RunMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn metrics_summary_serde_roundtrip() {
    let c = MetricsCollector::new();
    c.record(mk_run("a", 50, 100, 200, 0));
    c.record(mk_run("b", 150, 50, 100, 1));
    let s = c.summary();
    let json = serde_json::to_string(&s).unwrap();
    let s2: MetricsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn telemetry_span_serde_roundtrip() {
    let span = TelemetrySpan::new("serde_span")
        .with_attribute("key", "val")
        .with_attribute("run_id", "wo-1");
    let json = serde_json::to_string(&span).unwrap();
    let span2: TelemetrySpan = serde_json::from_str(&json).unwrap();
    assert_eq!(span2.name, "serde_span");
    assert_eq!(span2.attributes["key"], "val");
    assert_eq!(span2.attributes["run_id"], "wo-1");
}

#[test]
fn run_summary_serde_roundtrip() {
    let s = RunSummary::from_events(&["tool_call", "error", "message"], 42);
    let json = serde_json::to_string(&s).unwrap();
    let s2: RunSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, s2);
}

#[test]
fn latency_histogram_serde_roundtrip() {
    let mut h = LatencyHistogram::new();
    h.record(1.0);
    h.record(2.5);
    h.record(3.7);
    let json = serde_json::to_string(&h).unwrap();
    let h2: LatencyHistogram = serde_json::from_str(&json).unwrap();
    assert_eq!(h, h2);
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
fn request_key_serde_roundtrip() {
    let key = RequestKey {
        backend: "mock".into(),
        dialect: "openai".into(),
        outcome: "success".into(),
    };
    let json = serde_json::to_string(&key).unwrap();
    let key2: RequestKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, key2);
}

#[test]
fn model_pricing_serde_roundtrip() {
    let p = ModelPricing {
        input_cost_per_token: 0.00001,
        output_cost_per_token: 0.00003,
    };
    let json = serde_json::to_string(&p).unwrap();
    let p2: ModelPricing = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn telemetry_event_type_serde_roundtrip() {
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
    for t in types {
        let json = serde_json::to_string(&t).unwrap();
        let t2: TelemetryEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(t, t2);
    }
}

#[test]
fn metrics_exporter_json_output() {
    let c = MetricsCollector::new();
    c.record(mk_run("mock", 100, 50, 100, 0));
    let s = c.summary();
    let json = MetricsExporter::export_json(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 1);
}

#[test]
fn metrics_exporter_csv_output() {
    let runs = vec![mk_run("mock", 100, 50, 100, 0)];
    let csv = MetricsExporter::export_csv(&runs).unwrap();
    assert!(csv.contains("backend_name"));
    assert!(csv.contains("mock"));
}

#[test]
fn metrics_exporter_structured_output() {
    let c = MetricsCollector::new();
    c.record(mk_run("mock", 100, 50, 100, 0));
    let s = c.summary();
    let out = MetricsExporter::export_structured(&s).unwrap();
    assert!(out.contains("count=1"));
    assert!(out.contains("backend.mock=1"));
}

// =========================================================================
// 14. Edge cases
// =========================================================================

#[test]
fn no_hooks_fire_without_panic() {
    let reg = HookRegistry::new();
    reg.fire_pre("wo-none", "mock");
    reg.fire_post("wo-none", &RequestOutcome::Success, 0);
    reg.fire_event("tool_call");
    reg.fire_error("E001", "msg", ErrorClassification::Unknown);
}

#[test]
fn safe_registry_no_hooks_returns_zero() {
    let reg = SafeHookRegistry::new();
    assert_eq!(reg.fire_pre_safe("wo-none", "mock"), 0);
    assert_eq!(reg.fire_event_safe("tool_call"), 0);
    assert_eq!(
        reg.fire_post_safe("wo-none", &RequestOutcome::Success, 0),
        0
    );
    assert_eq!(
        reg.fire_error_safe("E001", "msg", ErrorClassification::Unknown),
        0
    );
}

#[test]
fn duplicate_hook_registration() {
    let count = Arc::new(Mutex::new(0u32));
    let mut reg = HookRegistry::new();
    let c1 = count.clone();
    let hook = move |_: &str, _: &str| {
        *c1.lock().unwrap() += 1;
    };
    // Registering the "same" closure multiple times creates distinct entries.
    reg.add_pre_hook(hook);
    let c2 = count.clone();
    reg.add_pre_hook(move |_, _| {
        *c2.lock().unwrap() += 1;
    });
    reg.fire_pre("wo-dup", "mock");
    assert_eq!(*count.lock().unwrap(), 2);
}

#[test]
fn empty_work_order_id() {
    let start = on_request_start("", "mock");
    let elapsed = on_request_complete("", "mock", &RequestOutcome::Success, start);
    assert!(elapsed < 1000);
}

#[test]
fn empty_backend_name() {
    let start = on_request_start("wo-1", "");
    let elapsed = on_request_complete("wo-1", "", &RequestOutcome::Success, start);
    assert!(elapsed < 1000);
}

#[test]
fn empty_error_code_and_message() {
    on_error("", "", "", ErrorClassification::Unknown);
}

#[test]
fn run_summary_no_events() {
    let s = RunSummary::new();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.error_count, 0);
    assert!(!s.has_errors());
    assert_eq!(s.error_rate(), 0.0);
}

#[test]
fn run_summary_from_empty_slice() {
    let s = RunSummary::from_events(&[], 0);
    assert_eq!(s.total_events, 0);
    assert_eq!(s.total_duration_ms, 0);
}

#[test]
fn latency_histogram_empty() {
    let h = LatencyHistogram::new();
    assert!(h.is_empty());
    assert_eq!(h.count(), 0);
    assert_eq!(h.min(), None);
    assert_eq!(h.max(), None);
    assert_eq!(h.mean(), 0.0);
    assert_eq!(h.p50(), 0.0);
    assert_eq!(h.p95(), 0.0);
    assert_eq!(h.p99(), 0.0);
}

#[test]
fn latency_histogram_single_value() {
    let mut h = LatencyHistogram::new();
    h.record(42.0);
    assert_eq!(h.count(), 1);
    assert_eq!(h.min(), Some(42.0));
    assert_eq!(h.max(), Some(42.0));
    assert_eq!(h.mean(), 42.0);
    assert_eq!(h.p50(), 42.0);
    assert_eq!(h.p99(), 42.0);
}

#[test]
fn collector_empty_summary() {
    let c = MetricsCollector::new();
    let s = c.summary();
    assert_eq!(s.count, 0);
    assert_eq!(s.mean_duration_ms, 0.0);
    assert!(s.backend_counts.is_empty());
}

#[test]
fn telemetry_collector_empty_summary() {
    let tc = TelemetryCollector::new();
    let s = tc.summary();
    assert_eq!(s.total_events, 0);
    assert!(s.events_by_type.is_empty());
    assert_eq!(s.average_run_duration_ms, None);
    assert_eq!(s.error_rate, 0.0);
}

#[test]
fn telemetry_collector_clear() {
    let mut tc = TelemetryCollector::new();
    tc.record(mk_event(TelemetryEventType::RunStarted, None, None, None));
    tc.clear();
    assert_eq!(tc.events().len(), 0);
}

#[test]
fn cost_estimator_unknown_model() {
    let est = CostEstimator::new();
    assert!(est.estimate("unknown", 100, 200).is_none());
}

#[test]
fn cost_estimator_known_model() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let cost = est.estimate("gpt-4", 1000, 500).unwrap();
    let expected = 1000.0 * 0.00003 + 500.0 * 0.00006;
    assert!((cost - expected).abs() < 1e-10);
}

#[test]
fn cost_estimator_total_skips_unknown() {
    let mut est = CostEstimator::new();
    est.set_pricing(
        "gpt-4",
        ModelPricing {
            input_cost_per_token: 0.00003,
            output_cost_per_token: 0.00006,
        },
    );
    let total = est.estimate_total(&[("gpt-4", 1000, 500), ("unknown", 100, 200)]);
    let expected = 1000.0 * 0.00003 + 500.0 * 0.00006;
    assert!((total - expected).abs() < 1e-10);
}

#[test]
fn metrics_exporter_format_dispatch() {
    let c = MetricsCollector::new();
    c.record(mk_run("mock", 100, 50, 100, 0));
    let s = c.summary();
    let json = MetricsExporter::export(&s, ExportFormat::Json).unwrap();
    assert!(json.contains("count"));
    let csv = MetricsExporter::export(&s, ExportFormat::Csv).unwrap();
    assert!(csv.contains("count"));
    let structured = MetricsExporter::export(&s, ExportFormat::Structured).unwrap();
    assert!(structured.contains("count="));
}

#[test]
fn json_exporter_trait_impl() {
    let exporter = JsonExporter;
    let s = MetricsSummary::default();
    let json = exporter.export(&s).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["count"], 0);
}

#[test]
fn concurrent_metrics_collector() {
    let c = MetricsCollector::new();
    let mut handles = vec![];
    for i in 0..20 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.record(mk_run("thread", i * 10, 10, 20, 0));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.len(), 20);
    let s = c.summary();
    assert_eq!(s.count, 20);
}

#[test]
fn hook_lifecycle_integration() {
    let pre_called = Arc::new(Mutex::new(false));
    let post_elapsed = Arc::new(Mutex::new(0u64));
    let event_count = Arc::new(Mutex::new(0u32));
    let error_codes = Arc::new(Mutex::new(Vec::new()));

    let mut reg = HookRegistry::new();

    let pc = pre_called.clone();
    reg.add_pre_hook(move |_, _| {
        *pc.lock().unwrap() = true;
    });

    let pe = post_elapsed.clone();
    reg.add_post_hook(move |_, _, elapsed| {
        *pe.lock().unwrap() = elapsed;
    });

    let ec = event_count.clone();
    reg.add_event_hook(move |_| {
        *ec.lock().unwrap() += 1;
    });

    let er = error_codes.clone();
    reg.add_error_hook(move |code, _, _| {
        er.lock().unwrap().push(code.to_string());
    });

    // Simulate full lifecycle
    reg.fire_pre("wo-lifecycle", "mock");
    let start = on_request_start("wo-lifecycle", "mock");

    reg.fire_event("tool_call");
    reg.fire_event("message");
    reg.fire_error("E001", "temp error", ErrorClassification::Transient);

    let elapsed = on_request_complete("wo-lifecycle", "mock", &RequestOutcome::Success, start);
    reg.fire_post("wo-lifecycle", &RequestOutcome::Success, elapsed);

    assert!(*pre_called.lock().unwrap());
    assert!(*post_elapsed.lock().unwrap() > 0 || elapsed == 0);
    assert_eq!(*event_count.lock().unwrap(), 2);
    assert_eq!(*error_codes.lock().unwrap(), vec!["E001"]);
}

#[test]
fn timing_monotonicity() {
    let start = Instant::now();
    let hook_start = on_request_start("wo-time", "mock");
    thread::sleep(Duration::from_millis(5));
    let elapsed = on_request_complete("wo-time", "mock", &RequestOutcome::Success, hook_start);
    let total = start.elapsed().as_millis() as u64;
    assert!(elapsed >= 5);
    assert!(elapsed <= total);
}
