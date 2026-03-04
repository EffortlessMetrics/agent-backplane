// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the metrics, spans, and hooks modules.
#![allow(clippy::float_cmp)]

use abp_telemetry::hooks::{
    ErrorClassification, RequestOutcome, on_error, on_request_complete, on_request_start,
};
use abp_telemetry::metrics::{ActiveRequestGauge, ErrorCounter, RequestCounter, TokenAccumulator};
use abp_telemetry::spans::{backend_span, event_span, request_span};

use std::sync::Arc;
use std::thread;

// =========================================================================
// RequestCounter
// =========================================================================

#[test]
fn request_counter_multiple_backends() {
    let c = RequestCounter::new();
    c.increment("openai", "openai", "success");
    c.increment("openai", "openai", "success");
    c.increment("anthropic", "anthropic", "success");
    c.increment("anthropic", "anthropic", "error");
    assert_eq!(c.get("openai", "openai", "success"), 2);
    assert_eq!(c.get("anthropic", "anthropic", "success"), 1);
    assert_eq!(c.get("anthropic", "anthropic", "error"), 1);
    assert_eq!(c.total(), 4);
}

#[test]
fn request_counter_concurrent() {
    let c = RequestCounter::new();
    let mut handles = vec![];
    for i in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.increment("b", "d", &format!("outcome_{}", i % 2));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.total(), 10);
}

#[test]
fn request_counter_snapshot_is_independent() {
    let c = RequestCounter::new();
    c.increment("a", "d", "ok");
    let snap = c.snapshot();
    c.increment("a", "d", "ok");
    // Snapshot should not reflect the second increment.
    assert_eq!(snap.values().sum::<u64>(), 1);
    assert_eq!(c.total(), 2);
}

// =========================================================================
// ErrorCounter
// =========================================================================

#[test]
fn error_counter_multiple_codes() {
    let c = ErrorCounter::new();
    c.increment("timeout");
    c.increment("timeout");
    c.increment("rate_limit");
    assert_eq!(c.get("timeout"), 2);
    assert_eq!(c.get("rate_limit"), 1);
    assert_eq!(c.total(), 3);
}

#[test]
fn error_counter_snapshot() {
    let c = ErrorCounter::new();
    c.increment("E001");
    c.increment("E002");
    let snap = c.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap["E001"], 1);
}

#[test]
fn error_counter_concurrent() {
    let c = ErrorCounter::new();
    let mut handles = vec![];
    for _ in 0..10 {
        let cc = c.clone();
        handles.push(thread::spawn(move || {
            cc.increment("E001");
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.get("E001"), 10);
}

// =========================================================================
// ActiveRequestGauge
// =========================================================================

#[test]
fn gauge_tracks_in_flight() {
    let g = Arc::new(ActiveRequestGauge::new());
    g.increment();
    g.increment();
    g.increment();
    assert_eq!(g.get(), 3);
    g.decrement();
    assert_eq!(g.get(), 2);
    g.decrement();
    g.decrement();
    assert_eq!(g.get(), 0);
}

#[test]
fn gauge_can_go_negative() {
    let g = ActiveRequestGauge::new();
    g.decrement();
    assert_eq!(g.get(), -1);
}

#[test]
fn gauge_concurrent() {
    let g = Arc::new(ActiveRequestGauge::new());
    let mut handles = vec![];
    for _ in 0..10 {
        let gg = Arc::clone(&g);
        handles.push(thread::spawn(move || {
            gg.increment();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(g.get(), 10);
}

// =========================================================================
// TokenAccumulator
// =========================================================================

#[test]
fn token_accumulator_accumulates() {
    let t = Arc::new(TokenAccumulator::new());
    t.add(100, 200);
    t.add(50, 75);
    assert_eq!(t.total_input(), 150);
    assert_eq!(t.total_output(), 275);
    assert_eq!(t.total(), 425);
}

#[test]
fn token_accumulator_concurrent() {
    let t = Arc::new(TokenAccumulator::new());
    let mut handles = vec![];
    for _ in 0..10 {
        let tt = Arc::clone(&t);
        handles.push(thread::spawn(move || {
            tt.add(10, 20);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(t.total_input(), 100);
    assert_eq!(t.total_output(), 200);
}

#[test]
fn token_accumulator_reset_clears() {
    let t = TokenAccumulator::new();
    t.add(100, 200);
    t.reset();
    assert_eq!(t.total_input(), 0);
    assert_eq!(t.total_output(), 0);
}

// =========================================================================
// Spans
// =========================================================================

#[test]
fn all_span_helpers_return_valid_spans() {
    let s1 = request_span("wo-1", "do stuff", "mapped");
    let s2 = event_span("tool_call", 42);
    let s3 = backend_span("sidecar:node");
    // Entering and exiting should not panic.
    let _g1 = s1.enter();
    let _g2 = s2.enter();
    let _g3 = s3.enter();
}

#[test]
fn spans_work_with_tracing_subscriber() {
    use std::sync::Mutex;

    #[derive(Clone)]
    struct Counter {
        count: Arc<Mutex<usize>>,
    }
    impl tracing::Subscriber for Counter {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            *self.count.lock().unwrap() += 1;
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let count = Arc::new(Mutex::new(0usize));
    let sub = Counter {
        count: count.clone(),
    };

    tracing::subscriber::with_default(sub, || {
        let s1 = request_span("wo-1", "task", "mapped");
        let _g1 = s1.enter();
        let s2 = event_span("tool_call", 1);
        let _g2 = s2.enter();
        let s3 = backend_span("mock");
        let _g3 = s3.enter();
    });

    assert_eq!(*count.lock().unwrap(), 3);
}

// =========================================================================
// Hooks
// =========================================================================

#[test]
fn hooks_full_lifecycle() {
    let start = on_request_start("wo-42", "mock");
    let elapsed = on_request_complete("wo-42", "mock", &RequestOutcome::Success, start);
    assert!(elapsed < 5000, "elapsed should be reasonable");
}

#[test]
fn hooks_error_lifecycle() {
    let start = on_request_start("wo-err", "sidecar:node");
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    let elapsed = on_request_complete("wo-err", "sidecar:node", &outcome, start);
    assert!(elapsed < 5000);
}

#[test]
fn hooks_on_error_all_classifications() {
    on_error(
        "wo-1",
        "E001",
        "transient error",
        ErrorClassification::Transient,
    );
    on_error(
        "wo-2",
        "E002",
        "permanent error",
        ErrorClassification::Permanent,
    );
    on_error(
        "wo-3",
        "E003",
        "unknown error",
        ErrorClassification::Unknown,
    );
}

#[test]
fn hooks_timing_is_monotonic() {
    let start = on_request_start("wo-time", "mock");
    // Small sleep to ensure nonzero elapsed time.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let elapsed = on_request_complete("wo-time", "mock", &RequestOutcome::Success, start);
    assert!(elapsed >= 5);
}

// =========================================================================
// Combined: metrics + hooks together
// =========================================================================

#[test]
fn metrics_and_hooks_integration() {
    let req_counter = RequestCounter::new();
    let err_counter = ErrorCounter::new();
    let gauge = Arc::new(ActiveRequestGauge::new());
    let tokens = Arc::new(TokenAccumulator::new());

    // Simulate a successful request.
    gauge.increment();
    let start = on_request_start("wo-int-1", "mock");
    tokens.add(500, 1000);
    let _elapsed = on_request_complete("wo-int-1", "mock", &RequestOutcome::Success, start);
    req_counter.increment("mock", "openai", "success");
    gauge.decrement();

    // Simulate a failed request.
    gauge.increment();
    let start = on_request_start("wo-int-2", "sidecar:node");
    on_error(
        "wo-int-2",
        "timeout",
        "deadline exceeded",
        ErrorClassification::Transient,
    );
    err_counter.increment("timeout");
    let outcome = RequestOutcome::Error {
        code: "timeout".into(),
        message: "deadline exceeded".into(),
    };
    let _elapsed = on_request_complete("wo-int-2", "sidecar:node", &outcome, start);
    req_counter.increment("sidecar:node", "anthropic", "error");
    gauge.decrement();

    assert_eq!(req_counter.total(), 2);
    assert_eq!(err_counter.total(), 1);
    assert_eq!(gauge.get(), 0);
    assert_eq!(tokens.total_input(), 500);
    assert_eq!(tokens.total_output(), 1000);
}
