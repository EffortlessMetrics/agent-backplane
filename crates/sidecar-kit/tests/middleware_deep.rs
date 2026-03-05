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
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep tests for the sidecar-kit middleware chain – covering both
//! value-based (`middleware`) and typed (`typed_middleware`) systems,
//! plus the `pipeline` and `transform` layers.

use serde_json::{json, Value};
use sidecar_kit::middleware::{
    ErrorWrapMiddleware, EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain,
    TimingMiddleware,
};
use sidecar_kit::pipeline::{
    EventPipeline, PipelineError, PipelineStage, RedactStage, TimestampStage, ValidateStage,
};

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn run_started() -> Value {
    json!({"type": "run_started", "message": "starting"})
}

fn assistant_msg() -> Value {
    json!({"type": "assistant_message", "text": "hello world"})
}

fn error_event() -> Value {
    json!({"type": "error", "message": "something broke"})
}

fn tool_call_event() -> Value {
    json!({"type": "tool_call", "tool_name": "grep", "input": {"q": "foo"}})
}

fn warning_event() -> Value {
    json!({"type": "warning", "message": "careful"})
}

/// Middleware that uppercases the `"message"` field.
struct UppercaseMiddleware;

impl EventMiddleware for UppercaseMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let mut out = event.clone();
        if let Some(msg) = out
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_uppercase)
        {
            out["message"] = Value::String(msg);
        }
        Some(out)
    }
}

/// Middleware that drops every event.
struct DropAllMiddleware;

impl EventMiddleware for DropAllMiddleware {
    fn process(&self, _event: &Value) -> Option<Value> {
        None
    }
}

/// Middleware that appends a `"tag"` field.
struct TagMiddleware(String);

impl EventMiddleware for TagMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let mut out = event.clone();
        out["tag"] = Value::String(self.0.clone());
        Some(out)
    }
}

/// Middleware that records how many times it was called.
struct CounterMiddleware {
    count: std::sync::atomic::AtomicUsize,
}

impl CounterMiddleware {
    fn new() -> Self {
        Self {
            count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn count(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl EventMiddleware for CounterMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(event.clone())
    }
}

/// Middleware that injects a sequence number.
struct SequenceMiddleware {
    seq: std::sync::atomic::AtomicU64,
}

impl SequenceMiddleware {
    fn new() -> Self {
        Self {
            seq: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl EventMiddleware for SequenceMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut out = event.clone();
        out["_seq"] = json!(n);
        Some(out)
    }
}

/// Middleware that returns Error action on error-type events.
struct ErrorIfErrorType;

impl EventMiddleware for ErrorIfErrorType {
    fn process(&self, event: &Value) -> Option<Value> {
        let t = event.get("type").and_then(Value::as_str).unwrap_or("");
        if t == "error" {
            None
        } else {
            Some(event.clone())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Middleware chain construction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_new_is_empty() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_default_is_empty() {
    let chain = MiddlewareChain::default();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_push_increments_len() {
    let mut chain = MiddlewareChain::new();
    chain.push(LoggingMiddleware::new());
    assert_eq!(chain.len(), 1);
    chain.push(LoggingMiddleware::new());
    assert_eq!(chain.len(), 2);
    assert!(!chain.is_empty());
}

#[test]
fn chain_with_builder_returns_self() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(TimingMiddleware::new())
        .with(ErrorWrapMiddleware::new());
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_with_many_middlewares() {
    let mut chain = MiddlewareChain::new();
    for i in 0..100 {
        chain.push(TagMiddleware(format!("m{i}")));
    }
    assert_eq!(chain.len(), 100);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Request interception (filter / drop)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn filter_include_intercepts_non_matching() {
    let filter = FilterMiddleware::include_kinds(&["tool_call"]);
    assert!(filter.process(&tool_call_event()).is_some());
    assert!(filter.process(&run_started()).is_none());
    assert!(filter.process(&error_event()).is_none());
}

#[test]
fn filter_exclude_intercepts_matching() {
    let filter = FilterMiddleware::exclude_kinds(&["error", "warning"]);
    assert!(filter.process(&error_event()).is_none());
    assert!(filter.process(&warning_event()).is_none());
    assert!(filter.process(&run_started()).is_some());
}

#[test]
fn filter_multiple_include_kinds() {
    let filter = FilterMiddleware::include_kinds(&["run_started", "error"]);
    assert!(filter.process(&run_started()).is_some());
    assert!(filter.process(&error_event()).is_some());
    assert!(filter.process(&assistant_msg()).is_none());
    assert!(filter.process(&tool_call_event()).is_none());
}

#[test]
fn chain_intercepts_before_transform() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["error"]))
        .with(UppercaseMiddleware);

    // run_started is intercepted by filter, never reaches uppercase
    assert!(chain.process(&run_started()).is_none());
    // error passes through filter, gets uppercased
    let result = chain.process(&error_event()).unwrap();
    assert_eq!(result["message"], "SOMETHING BROKE");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Response transformation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn uppercase_transforms_message() {
    let mw = UppercaseMiddleware;
    let result = mw.process(&run_started()).unwrap();
    assert_eq!(result["message"], "STARTING");
}

#[test]
fn tag_transforms_by_adding_field() {
    let mw = TagMiddleware("v1".into());
    let result = mw.process(&run_started()).unwrap();
    assert_eq!(result["tag"], "v1");
    assert_eq!(result["type"], "run_started");
}

#[test]
fn timing_adds_processing_field() {
    let mw = TimingMiddleware::new();
    let result = mw.process(&run_started()).unwrap();
    assert!(result.get("_processing_us").is_some());
    assert!(result["_processing_us"].is_number());
}

#[test]
fn chain_transforms_accumulate() {
    let chain = MiddlewareChain::new()
        .with(TagMiddleware("first".into()))
        .with(UppercaseMiddleware)
        .with(TimingMiddleware::new());

    let result = chain.process(&run_started()).unwrap();
    assert_eq!(result["tag"], "first");
    assert_eq!(result["message"], "STARTING");
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn transform_preserves_unrelated_fields() {
    let event = json!({"type": "run_started", "message": "go", "extra": 42, "nested": {"a": 1}});
    let chain = MiddlewareChain::new().with(UppercaseMiddleware);
    let result = chain.process(&event).unwrap();
    assert_eq!(result["extra"], 42);
    assert_eq!(result["nested"]["a"], 1);
    assert_eq!(result["message"], "GO");
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Error handling middleware
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_wrap_passes_object_through() {
    let mw = ErrorWrapMiddleware::new();
    let event = run_started();
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

#[test]
fn error_wrap_wraps_string_value() {
    let mw = ErrorWrapMiddleware::new();
    let event = json!("raw string");
    let result = mw.process(&event).unwrap();
    assert_eq!(result["type"], "error");
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("non-object event replaced"));
    assert_eq!(result["_original"], "raw string");
}

#[test]
fn error_wrap_wraps_number_value() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!(42)).unwrap();
    assert_eq!(result["type"], "error");
    assert_eq!(result["_original"], 42);
}

#[test]
fn error_wrap_wraps_array_value() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!([1, 2, 3])).unwrap();
    assert_eq!(result["type"], "error");
    assert_eq!(result["_original"], json!([1, 2, 3]));
}

#[test]
fn error_wrap_wraps_null_value() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!(null)).unwrap();
    assert_eq!(result["type"], "error");
}

#[test]
fn error_wrap_wraps_bool_value() {
    let mw = ErrorWrapMiddleware::new();
    let result = mw.process(&json!(true)).unwrap();
    assert_eq!(result["type"], "error");
    assert_eq!(result["_original"], true);
}

#[test]
fn error_wrap_in_chain_normalizes_input() {
    let chain = MiddlewareChain::new()
        .with(ErrorWrapMiddleware::new())
        .with(UppercaseMiddleware);

    // Non-object gets wrapped into error object, then uppercased
    let result = chain.process(&json!("oops")).unwrap();
    assert_eq!(result["type"], "error");
    let msg = result["message"].as_str().unwrap();
    assert!(msg.starts_with("NON-OBJECT EVENT REPLACED"));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Logging middleware
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn logging_passes_through_unchanged() {
    let mw = LoggingMiddleware::new();
    let event = assistant_msg();
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

#[test]
fn logging_default_passes_through() {
    let mw = LoggingMiddleware;
    let event = tool_call_event();
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

#[test]
fn logging_handles_non_object_events() {
    let mw = LoggingMiddleware::new();
    let result = mw.process(&json!(42)).unwrap();
    assert_eq!(result, json!(42));
}

#[test]
fn logging_handles_null_event() {
    let mw = LoggingMiddleware::new();
    let result = mw.process(&json!(null)).unwrap();
    assert_eq!(result, json!(null));
}

#[test]
fn logging_in_chain_is_transparent() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(TagMiddleware("x".into()))
        .with(LoggingMiddleware::new());

    let result = chain.process(&run_started()).unwrap();
    assert_eq!(result["tag"], "x");
    assert_eq!(result["message"], "starting");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Retry middleware (simulated via counter + re-process)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_pattern_reprocesses_events() {
    // Simulate a retry pattern: if first processing drops, try again
    let filter = FilterMiddleware::include_kinds(&["run_started"]);
    let event = error_event();

    let mut attempts = 0;
    let mut result = filter.process(&event);
    while result.is_none() && attempts < 3 {
        attempts += 1;
        result = filter.process(&event);
    }
    // error_event never matches include_kinds(["run_started"]), so all retries fail
    assert_eq!(attempts, 3);
    assert!(result.is_none());
}

#[test]
fn retry_succeeds_on_matching_event() {
    let filter = FilterMiddleware::include_kinds(&["run_started"]);
    let event = run_started();

    let result = filter.process(&event);
    assert!(result.is_some());
}

#[test]
fn counter_middleware_tracks_retries() {
    let counter = CounterMiddleware::new();
    let event = run_started();
    for _ in 0..5 {
        counter.process(&event);
    }
    assert_eq!(counter.count(), 5);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Timeout middleware (simulated via timing)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn timing_middleware_records_processing_time() {
    let mw = TimingMiddleware::new();
    let result = mw.process(&run_started()).unwrap();
    let us = result["_processing_us"].as_u64().unwrap();
    // Processing should be very fast (microseconds)
    assert!(us < 1_000_000, "expected sub-second processing, got {us}µs");
}

#[test]
fn timing_middleware_on_non_object_skips_field() {
    let mw = TimingMiddleware::new();
    let result = mw.process(&json!("string value")).unwrap();
    // Non-object: cannot insert field, so no _processing_us
    assert!(result.get("_processing_us").is_none());
    assert_eq!(result, json!("string value"));
}

#[test]
fn timing_middleware_default_constructor() {
    let mw = TimingMiddleware;
    let result = mw.process(&json!({"type": "test"})).unwrap();
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn timing_with_heavy_chain_stays_reasonable() {
    let mut chain = MiddlewareChain::new();
    chain.push(TimingMiddleware::new());
    // Add 50 passthrough middlewares
    for _ in 0..50 {
        chain.push(LoggingMiddleware::new());
    }

    let event = json!({"type": "run_started", "message": "go"});
    let result = chain.process(&event).unwrap();
    // Timing was captured at the first middleware, should still be fast
    let us = result["_processing_us"].as_u64().unwrap();
    assert!(us < 10_000_000, "expected <10s, got {us}µs");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Rate limiting middleware (value-based simulation)
// ═══════════════════════════════════════════════════════════════════════

/// Value-based rate limiter that allows N events then drops the rest.
struct ValueRateLimiter {
    max: usize,
    count: std::sync::atomic::AtomicUsize,
}

impl ValueRateLimiter {
    fn new(max: usize) -> Self {
        Self {
            max,
            count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl EventMiddleware for ValueRateLimiter {
    fn process(&self, event: &Value) -> Option<Value> {
        let n = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n < self.max {
            Some(event.clone())
        } else {
            None
        }
    }
}

#[test]
fn rate_limiter_allows_up_to_max() {
    let limiter = ValueRateLimiter::new(5);
    let event = run_started();
    let mut passed = 0;
    for _ in 0..10 {
        if limiter.process(&event).is_some() {
            passed += 1;
        }
    }
    assert_eq!(passed, 5);
}

#[test]
fn rate_limiter_zero_drops_all() {
    let limiter = ValueRateLimiter::new(0);
    assert!(limiter.process(&run_started()).is_none());
}

#[test]
fn rate_limiter_in_chain() {
    let chain = MiddlewareChain::new()
        .with(ValueRateLimiter::new(3))
        .with(TagMiddleware("ok".into()));

    let event = run_started();
    let mut tagged = 0;
    for _ in 0..5 {
        if chain.process(&event).is_some() {
            tagged += 1;
        }
    }
    assert_eq!(tagged, 3);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Middleware ordering matters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn filter_before_transform_drops_early() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["error"]))
        .with(UppercaseMiddleware);

    assert!(chain.process(&run_started()).is_none());
    let e = chain.process(&error_event()).unwrap();
    assert_eq!(e["message"], "SOMETHING BROKE");
}

#[test]
fn transform_before_filter_processes_then_drops() {
    let chain = MiddlewareChain::new()
        .with(UppercaseMiddleware)
        .with(FilterMiddleware::include_kinds(&["error"]));

    // run_started is uppercased, but then filter drops it
    assert!(chain.process(&run_started()).is_none());
    let e = chain.process(&error_event()).unwrap();
    assert_eq!(e["message"], "SOMETHING BROKE");
}

#[test]
fn error_wrap_before_filter_changes_behavior() {
    // non-object → error_wrap makes it {"type":"error", ...} → filter passes
    let chain_wrap_first = MiddlewareChain::new()
        .with(ErrorWrapMiddleware::new())
        .with(FilterMiddleware::include_kinds(&["error"]));

    let result = chain_wrap_first.process(&json!("oops"));
    assert!(result.is_some());

    // filter first → non-object has no "type" → dropped by include filter
    let chain_filter_first = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["error"]))
        .with(ErrorWrapMiddleware::new());

    let result = chain_filter_first.process(&json!("oops"));
    assert!(result.is_none());
}

#[test]
fn tag_order_visible_in_output() {
    let chain = MiddlewareChain::new()
        .with(TagMiddleware("alpha".into()))
        .with(TagMiddleware("beta".into()));

    let result = chain.process(&run_started()).unwrap();
    // Second tag overwrites first
    assert_eq!(result["tag"], "beta");
}

#[test]
fn sequence_middleware_assigns_monotonic_ids() {
    let seq = SequenceMiddleware::new();
    let e0 = seq.process(&run_started()).unwrap();
    let e1 = seq.process(&run_started()).unwrap();
    let e2 = seq.process(&run_started()).unwrap();
    assert_eq!(e0["_seq"], 0);
    assert_eq!(e1["_seq"], 1);
    assert_eq!(e2["_seq"], 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Empty middleware chain is passthrough
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_chain_passes_object() {
    let chain = MiddlewareChain::new();
    let event = run_started();
    assert_eq!(chain.process(&event).unwrap(), event);
}

#[test]
fn empty_chain_passes_string() {
    let chain = MiddlewareChain::new();
    let event = json!("just a string");
    assert_eq!(chain.process(&event).unwrap(), event);
}

#[test]
fn empty_chain_passes_number() {
    let chain = MiddlewareChain::new();
    assert_eq!(chain.process(&json!(99)).unwrap(), json!(99));
}

#[test]
fn empty_chain_passes_null() {
    let chain = MiddlewareChain::new();
    assert_eq!(chain.process(&json!(null)).unwrap(), json!(null));
}

#[test]
fn empty_chain_passes_complex_nested() {
    let chain = MiddlewareChain::new();
    let event = json!({"a": [1, {"b": true}], "c": null});
    assert_eq!(chain.process(&event).unwrap(), event);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Middleware composition
// ═══════════════════════════════════════════════════════════════════════

/// Adapter: wrap a MiddlewareChain as a single EventMiddleware.
struct ChainMiddleware(MiddlewareChain);

impl EventMiddleware for ChainMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        self.0.process(event)
    }
}

#[test]
fn nested_chains_compose() {
    let inner = MiddlewareChain::new()
        .with(TagMiddleware("inner".into()))
        .with(UppercaseMiddleware);

    let outer = MiddlewareChain::new()
        .with(ChainMiddleware(inner))
        .with(TimingMiddleware::new());

    let result = outer.process(&run_started()).unwrap();
    assert_eq!(result["tag"], "inner");
    assert_eq!(result["message"], "STARTING");
    assert!(result.get("_processing_us").is_some());
}

#[test]
fn triple_nested_chains() {
    let l1 = MiddlewareChain::new().with(TagMiddleware("l1".into()));
    let l2 = MiddlewareChain::new()
        .with(ChainMiddleware(l1))
        .with(TagMiddleware("l2".into()));
    let l3 = MiddlewareChain::new()
        .with(ChainMiddleware(l2))
        .with(TagMiddleware("l3".into()));

    let result = l3.process(&run_started()).unwrap();
    // l3's tag is last, so it wins
    assert_eq!(result["tag"], "l3");
}

#[test]
fn filter_inside_nested_chain_drops_event() {
    let inner = MiddlewareChain::new().with(FilterMiddleware::include_kinds(&["error"]));

    let outer = MiddlewareChain::new()
        .with(ChainMiddleware(inner))
        .with(TagMiddleware("seen".into()));

    // run_started dropped by inner filter
    assert!(outer.process(&run_started()).is_none());
    // error passes
    let result = outer.process(&error_event()).unwrap();
    assert_eq!(result["tag"], "seen");
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Async middleware (concurrent access)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn async_concurrent_processing() {
    use std::sync::Arc;

    let chain = Arc::new(
        MiddlewareChain::new()
            .with(UppercaseMiddleware)
            .with(TimingMiddleware::new()),
    );

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let chain = Arc::clone(&chain);
            tokio::spawn(async move {
                let event = json!({"type": "run_started", "message": format!("msg-{i}")});
                chain.process(&event)
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        let msg = result["message"].as_str().unwrap();
        assert!(msg.starts_with("MSG-"), "expected uppercased, got {msg}");
        assert!(result.get("_processing_us").is_some());
    }
}

#[tokio::test]
async fn async_counter_is_atomic() {
    use std::sync::Arc;

    let counter = Arc::new(CounterMiddleware::new());
    let handles: Vec<_> = (0..50)
        .map(|_| {
            let c = Arc::clone(&counter);
            tokio::spawn(async move {
                c.process(&run_started());
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(counter.count(), 50);
}

#[tokio::test]
async fn async_filter_concurrent_mixed_events() {
    use std::sync::Arc;

    let chain = Arc::new(
        MiddlewareChain::new().with(FilterMiddleware::exclude_kinds(&["error", "warning"])),
    );

    let handles: Vec<_> = (0..30)
        .map(|i| {
            let chain = Arc::clone(&chain);
            tokio::spawn(async move {
                let event = match i % 3 {
                    0 => run_started(),
                    1 => error_event(),
                    _ => warning_event(),
                };
                chain.process(&event)
            })
        })
        .collect();

    let mut passed = 0usize;
    let mut dropped = 0usize;
    for h in handles {
        match h.await.unwrap() {
            Some(_) => passed += 1,
            None => dropped += 1,
        }
    }
    assert_eq!(passed, 10); // every 3rd is run_started
    assert_eq!(dropped, 20);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Middleware state management
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn counter_tracks_state_across_calls() {
    let counter = CounterMiddleware::new();
    assert_eq!(counter.count(), 0);
    counter.process(&run_started());
    assert_eq!(counter.count(), 1);
    counter.process(&error_event());
    assert_eq!(counter.count(), 2);
    counter.process(&assistant_msg());
    assert_eq!(counter.count(), 3);
}

#[test]
fn sequence_state_persists() {
    let seq = SequenceMiddleware::new();
    for i in 0..10 {
        let result = seq.process(&run_started()).unwrap();
        assert_eq!(result["_seq"].as_u64().unwrap(), i);
    }
}

#[test]
fn rate_limiter_state_persists() {
    let limiter = ValueRateLimiter::new(2);
    assert!(limiter.process(&run_started()).is_some());
    assert!(limiter.process(&run_started()).is_some());
    assert!(limiter.process(&run_started()).is_none());
    assert!(limiter.process(&run_started()).is_none());
}

#[test]
fn stateful_middleware_in_shared_chain() {
    use std::sync::Arc;

    let chain = Arc::new(MiddlewareChain::new().with(ValueRateLimiter::new(3)));
    let chain2 = Arc::clone(&chain);

    // First 3 pass, rest drop
    assert!(chain.process(&run_started()).is_some());
    assert!(chain2.process(&run_started()).is_some());
    assert!(chain.process(&run_started()).is_some());
    assert!(chain2.process(&run_started()).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Middleware error propagation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn drop_middleware_propagates_none_through_chain() {
    let chain = MiddlewareChain::new()
        .with(DropAllMiddleware)
        .with(TagMiddleware("unreachable".into()));

    assert!(chain.process(&run_started()).is_none());
    assert!(chain.process(&error_event()).is_none());
    assert!(chain.process(&json!("anything")).is_none());
}

#[test]
fn error_if_error_type_drops_errors() {
    let chain = MiddlewareChain::new()
        .with(ErrorIfErrorType)
        .with(TagMiddleware("ok".into()));

    assert!(chain.process(&error_event()).is_none());
    let result = chain.process(&run_started()).unwrap();
    assert_eq!(result["tag"], "ok");
}

#[test]
fn error_wrap_then_error_filter_propagates() {
    // Wrap non-objects to error → then filter drops errors
    let chain = MiddlewareChain::new()
        .with(ErrorWrapMiddleware::new())
        .with(ErrorIfErrorType);

    // Non-object → wrapped as error → dropped
    assert!(chain.process(&json!("bad")).is_none());
    // Object that is error → dropped
    assert!(chain.process(&error_event()).is_none());
    // Normal object → passes
    assert!(chain.process(&run_started()).is_some());
}

#[test]
fn multiple_filters_compound() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::exclude_kinds(&["error"]))
        .with(FilterMiddleware::exclude_kinds(&["warning"]));

    assert!(chain.process(&error_event()).is_none());
    assert!(chain.process(&warning_event()).is_none());
    assert!(chain.process(&run_started()).is_some());
    assert!(chain.process(&assistant_msg()).is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Performance impact of middleware chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_chain_is_fast() {
    let chain = MiddlewareChain::new();
    let event = run_started();
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = chain.process(&event);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "1000 empty chain ops took {elapsed:?}"
    );
}

#[test]
fn long_chain_completes_in_time() {
    let mut chain = MiddlewareChain::new();
    for _ in 0..100 {
        chain.push(LoggingMiddleware::new());
    }
    let event = run_started();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let _ = chain.process(&event);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "100 events through 100 middlewares took {elapsed:?}"
    );
}

#[test]
fn short_circuit_is_faster_than_full_chain() {
    let event = run_started();
    // Chain that drops immediately
    let short = MiddlewareChain::new()
        .with(DropAllMiddleware)
        .with(LoggingMiddleware::new())
        .with(LoggingMiddleware::new())
        .with(LoggingMiddleware::new());

    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        let _ = short.process(&event);
    }
    let short_time = start.elapsed();

    // Chain that processes all
    let full = MiddlewareChain::new()
        .with(LoggingMiddleware::new())
        .with(LoggingMiddleware::new())
        .with(LoggingMiddleware::new())
        .with(LoggingMiddleware::new());

    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        let _ = full.process(&event);
    }
    let full_time = start.elapsed();

    // Short-circuit should be faster (or at least not dramatically slower)
    assert!(
        short_time <= full_time + std::time::Duration::from_millis(500),
        "short-circuit ({short_time:?}) should not be significantly slower than full ({full_time:?})"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Pipeline deep tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_empty_passthrough() {
    let pipeline = EventPipeline::new();
    let event = json!({"type": "test", "value": 123});
    let result = pipeline.process(event.clone()).unwrap().unwrap();
    assert_eq!(result, event);
}

#[test]
fn pipeline_default_passthrough() {
    let pipeline = EventPipeline::default();
    assert_eq!(pipeline.stage_count(), 0);
}

#[test]
fn pipeline_validate_rejects_missing_fields() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec![
        "type".into(),
        "id".into(),
    ])));

    let result = pipeline.process(json!({"type": "test"}));
    assert!(result.is_err());
}

#[test]
fn pipeline_validate_passes_present_fields() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));

    let event = json!({"type": "test", "data": 1});
    let result = pipeline.process(event.clone()).unwrap().unwrap();
    assert_eq!(result, event);
}

#[test]
fn pipeline_redact_removes_fields() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(RedactStage::new(vec![
        "secret".into(),
        "token".into(),
    ])));

    let event = json!({"type": "test", "secret": "abc", "token": "xyz", "data": 1});
    let result = pipeline.process(event).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("token").is_none());
    assert_eq!(result["data"], 1);
}

#[test]
fn pipeline_timestamp_adds_processed_at() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(TimestampStage::new()));

    let event = json!({"type": "test"});
    let result = pipeline.process(event).unwrap().unwrap();
    assert!(result.get("processed_at").is_some());
    assert!(result["processed_at"].is_number());
}

#[test]
fn pipeline_stages_compose() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));
    pipeline.add_stage(Box::new(RedactStage::new(vec!["secret".into()])));
    pipeline.add_stage(Box::new(TimestampStage::new()));

    assert_eq!(pipeline.stage_count(), 3);

    let event = json!({"type": "test", "secret": "s3cret", "data": 42});
    let result = pipeline.process(event).unwrap().unwrap();
    assert!(result.get("secret").is_none());
    assert!(result.get("processed_at").is_some());
    assert_eq!(result["data"], 42);
}

#[test]
fn pipeline_validate_non_object_returns_error() {
    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(ValidateStage::new(vec!["type".into()])));

    let result = pipeline.process(json!("not an object"));
    assert!(result.is_err());
}

#[test]
fn pipeline_drop_stage_filters() {
    struct DropStage;
    impl PipelineStage for DropStage {
        fn name(&self) -> &str {
            "drop"
        }
        fn process(&self, _event: Value) -> Result<Option<Value>, PipelineError> {
            Ok(None)
        }
    }

    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(DropStage));
    pipeline.add_stage(Box::new(TimestampStage::new()));

    let result = pipeline.process(json!({"type": "test"})).unwrap();
    assert!(result.is_none());
}

#[test]
fn pipeline_stage_error_propagates() {
    struct FailStage;
    impl PipelineStage for FailStage {
        fn name(&self) -> &str {
            "fail"
        }
        fn process(&self, _event: Value) -> Result<Option<Value>, PipelineError> {
            Err(PipelineError::StageError {
                stage: "fail".into(),
                message: "boom".into(),
            })
        }
    }

    let mut pipeline = EventPipeline::new();
    pipeline.add_stage(Box::new(FailStage));

    let result = pipeline.process(json!({"type": "test"}));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("boom"));
}

#[test]
fn pipeline_error_display() {
    let err = PipelineError::StageError {
        stage: "validate".into(),
        message: "missing field".into(),
    };
    assert_eq!(err.to_string(), "stage 'validate' failed: missing field");

    let err2 = PipelineError::InvalidEvent;
    assert_eq!(err2.to_string(), "event is not a valid JSON object");
}

// ═══════════════════════════════════════════════════════════════════════
// Typed middleware deep tests
// ═══════════════════════════════════════════════════════════════════════

mod typed_deep {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;
    use sidecar_kit::typed_middleware::{
        ErrorRecoveryMiddleware, FilterMiddleware as TypedFilter,
        LoggingMiddleware as TypedLogging, MetricsMiddleware, MiddlewareAction,
        RateLimitMiddleware, SidecarMiddleware, SidecarMiddlewareChain,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    fn run_started() -> AgentEvent {
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        })
    }

    fn assistant_msg() -> AgentEvent {
        make_event(AgentEventKind::AssistantMessage { text: "hi".into() })
    }

    fn error_ev() -> AgentEvent {
        make_event(AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        })
    }

    fn warning_ev() -> AgentEvent {
        make_event(AgentEventKind::Warning {
            message: "warn".into(),
        })
    }

    fn tool_call_ev() -> AgentEvent {
        make_event(AgentEventKind::ToolCall {
            tool_name: "grep".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"q": "foo"}),
        })
    }

    struct OrderMarker(String);
    impl SidecarMiddleware for OrderMarker {
        fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
            let ext = event.ext.get_or_insert_with(Default::default);
            let list = ext.entry("order".into()).or_insert(serde_json::json!([]));
            if let Some(arr) = list.as_array_mut() {
                arr.push(serde_json::json!(self.0));
            }
            MiddlewareAction::Continue
        }
    }

    struct CountingMiddleware {
        count: AtomicUsize,
    }
    impl CountingMiddleware {
        fn new() -> Self {
            Self {
                count: AtomicUsize::new(0),
            }
        }
        fn count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }
    impl SidecarMiddleware for CountingMiddleware {
        fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
            self.count.fetch_add(1, Ordering::SeqCst);
            MiddlewareAction::Continue
        }
    }

    // ── Typed chain construction ────────────────────────────────────

    #[test]
    fn typed_chain_new_is_empty() {
        let chain = SidecarMiddlewareChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn typed_chain_default_is_empty() {
        let chain = SidecarMiddlewareChain::default();
        assert!(chain.is_empty());
    }

    #[test]
    fn typed_chain_push_and_len() {
        let mut chain = SidecarMiddlewareChain::new();
        chain.push(TypedLogging::new());
        assert_eq!(chain.len(), 1);
        chain.push(TypedLogging::new());
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn typed_chain_with_builder() {
        let chain = SidecarMiddlewareChain::new()
            .with(TypedLogging::new())
            .with(MetricsMiddleware::new());
        assert_eq!(chain.len(), 2);
    }

    // ── Typed chain ordering ────────────────────────────────────────

    #[test]
    fn typed_chain_ordering_preserved() {
        let chain = SidecarMiddlewareChain::new()
            .with(OrderMarker("A".into()))
            .with(OrderMarker("B".into()))
            .with(OrderMarker("C".into()))
            .with(OrderMarker("D".into()));

        let mut event = run_started();
        chain.process(&mut event);
        let order = event.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order, vec!["A", "B", "C", "D"]);
    }

    // ── Typed skip short-circuits ───────────────────────────────────

    #[test]
    fn typed_skip_short_circuits() {
        struct SkipAll;
        impl SidecarMiddleware for SkipAll {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                MiddlewareAction::Skip
            }
        }

        let chain = SidecarMiddlewareChain::new()
            .with(OrderMarker("before".into()))
            .with(SkipAll)
            .with(OrderMarker("after".into()));

        let mut event = run_started();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Skip);
        let order = event.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order, vec!["before"]);
    }

    // ── Typed error action ──────────────────────────────────────────

    #[test]
    fn typed_error_short_circuits() {
        struct ErrorMw(String);
        impl SidecarMiddleware for ErrorMw {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                MiddlewareAction::Error(self.0.clone())
            }
        }

        let chain = SidecarMiddlewareChain::new()
            .with(ErrorMw("bad".into()))
            .with(OrderMarker("never".into()));

        let mut event = run_started();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Error("bad".into()));
        assert!(event.ext.is_none());
    }

    // ── Typed filter ────────────────────────────────────────────────

    #[test]
    fn typed_filter_drops_matching() {
        let filter =
            TypedFilter::new(|e: &AgentEvent| matches!(e.kind, AgentEventKind::Warning { .. }));

        let mut w = warning_ev();
        assert_eq!(filter.on_event(&mut w), MiddlewareAction::Skip);
        let mut r = run_started();
        assert_eq!(filter.on_event(&mut r), MiddlewareAction::Continue);
    }

    #[test]
    fn typed_filter_by_tool_name() {
        let filter = TypedFilter::new(
            |e: &AgentEvent| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "rm"),
        );

        let mut tc = tool_call_ev(); // tool_name = "grep"
        assert_eq!(filter.on_event(&mut tc), MiddlewareAction::Continue);
    }

    // ── Typed logging ───────────────────────────────────────────────

    #[test]
    fn typed_logging_continues() {
        let mw = TypedLogging::new();
        let mut ev = assistant_msg();
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
    }

    #[test]
    fn typed_logging_default() {
        let mw = TypedLogging;
        let mut ev = run_started();
        assert_eq!(mw.on_event(&mut ev), MiddlewareAction::Continue);
    }

    // ── Metrics ─────────────────────────────────────────────────────

    #[test]
    fn metrics_counts_by_kind() {
        let m = MetricsMiddleware::new();
        let mut e1 = run_started();
        let mut e2 = run_started();
        let mut e3 = error_ev();
        let mut e4 = assistant_msg();
        m.on_event(&mut e1);
        m.on_event(&mut e2);
        m.on_event(&mut e3);
        m.on_event(&mut e4);

        let c = m.counts();
        assert_eq!(c["run_started"], 2);
        assert_eq!(c["error"], 1);
        assert_eq!(c["assistant_message"], 1);
        assert_eq!(m.total(), 4);
    }

    #[test]
    fn metrics_records_timings() {
        let m = MetricsMiddleware::new();
        let mut ev = run_started();
        m.on_event(&mut ev);
        m.on_event(&mut ev);
        let timings = m.timings();
        assert_eq!(timings.len(), 2);
    }

    #[test]
    fn metrics_default() {
        let m = MetricsMiddleware::default();
        assert_eq!(m.total(), 0);
        assert!(m.counts().is_empty());
    }

    // ── Rate limiting ───────────────────────────────────────────────

    #[test]
    fn rate_limit_exact_boundary() {
        let rl = RateLimitMiddleware::new(5);
        let mut passed = 0;
        for _ in 0..5 {
            let mut ev = run_started();
            if rl.on_event(&mut ev) == MiddlewareAction::Continue {
                passed += 1;
            }
        }
        assert_eq!(passed, 5);
        // 6th should be skipped
        let mut ev = run_started();
        assert_eq!(rl.on_event(&mut ev), MiddlewareAction::Skip);
    }

    #[test]
    fn rate_limit_one_per_second() {
        let rl = RateLimitMiddleware::new(1);
        let mut ev = run_started();
        assert_eq!(rl.on_event(&mut ev), MiddlewareAction::Continue);
        assert_eq!(rl.on_event(&mut ev), MiddlewareAction::Skip);
        assert_eq!(rl.on_event(&mut ev), MiddlewareAction::Skip);
    }

    // ── Error recovery ──────────────────────────────────────────────

    #[test]
    fn error_recovery_catches_string_panic() {
        struct PanicStr;
        impl SidecarMiddleware for PanicStr {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                panic!("whoops");
            }
        }

        let wrapped = ErrorRecoveryMiddleware::wrap(PanicStr);
        let mut ev = run_started();
        let action = wrapped.on_event(&mut ev);
        assert_eq!(action, MiddlewareAction::Error("whoops".into()));
    }

    #[test]
    fn error_recovery_catches_string_owned_panic() {
        struct PanicOwned;
        impl SidecarMiddleware for PanicOwned {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                panic!("{}", "formatted panic");
            }
        }

        let wrapped = ErrorRecoveryMiddleware::wrap(PanicOwned);
        let mut ev = run_started();
        let action = wrapped.on_event(&mut ev);
        match action {
            MiddlewareAction::Error(msg) => {
                assert!(
                    msg.contains("formatted panic") || msg.contains("unknown"),
                    "unexpected error message: {msg}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn error_recovery_passes_through_normal() {
        let wrapped = ErrorRecoveryMiddleware::wrap(TypedLogging::new());
        let mut ev = run_started();
        assert_eq!(wrapped.on_event(&mut ev), MiddlewareAction::Continue);
    }

    #[test]
    fn error_recovery_in_chain() {
        struct PanicMw;
        impl SidecarMiddleware for PanicMw {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                panic!("inner panic");
            }
        }

        let chain = SidecarMiddlewareChain::new()
            .with(OrderMarker("pre".into()))
            .with(ErrorRecoveryMiddleware::wrap(PanicMw))
            .with(OrderMarker("post".into()));

        let mut ev = run_started();
        let action = chain.process(&mut ev);
        assert_eq!(action, MiddlewareAction::Error("inner panic".into()));
        // Pre-marker was applied, post was not
        let order = ev.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order, vec!["pre"]);
    }

    // ── Typed empty chain passthrough ───────────────────────────────

    #[test]
    fn typed_empty_chain_passthrough_run_started() {
        let chain = SidecarMiddlewareChain::new();
        let mut ev = run_started();
        assert_eq!(chain.process(&mut ev), MiddlewareAction::Continue);
        assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
    }

    #[test]
    fn typed_empty_chain_passthrough_error() {
        let chain = SidecarMiddlewareChain::new();
        let mut ev = error_ev();
        assert_eq!(chain.process(&mut ev), MiddlewareAction::Continue);
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }

    // ── Typed composition via subchain adapter ──────────────────────

    struct SubChain(SidecarMiddlewareChain);
    impl SidecarMiddleware for SubChain {
        fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
            self.0.process(event)
        }
    }

    #[test]
    fn typed_nested_composition() {
        let inner = SidecarMiddlewareChain::new()
            .with(OrderMarker("i1".into()))
            .with(OrderMarker("i2".into()));

        let outer = SidecarMiddlewareChain::new()
            .with(OrderMarker("o1".into()))
            .with(SubChain(inner))
            .with(OrderMarker("o2".into()));

        let mut ev = run_started();
        outer.process(&mut ev);
        let order = ev.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order, vec!["o1", "i1", "i2", "o2"]);
    }

    #[test]
    fn typed_nested_skip_propagates_out() {
        struct SkipAll;
        impl SidecarMiddleware for SkipAll {
            fn on_event(&self, _: &mut AgentEvent) -> MiddlewareAction {
                MiddlewareAction::Skip
            }
        }

        let inner = SidecarMiddlewareChain::new().with(SkipAll);
        let outer = SidecarMiddlewareChain::new()
            .with(SubChain(inner))
            .with(OrderMarker("never".into()));

        let mut ev = run_started();
        let action = outer.process(&mut ev);
        assert_eq!(action, MiddlewareAction::Skip);
        assert!(ev.ext.is_none());
    }

    // ── Typed state management ──────────────────────────────────────

    #[test]
    fn typed_counting_state() {
        let c = CountingMiddleware::new();
        assert_eq!(c.count(), 0);
        let mut ev = run_started();
        c.on_event(&mut ev);
        c.on_event(&mut ev);
        c.on_event(&mut ev);
        assert_eq!(c.count(), 3);
    }

    // ── Typed concurrent usage ──────────────────────────────────────

    #[tokio::test]
    async fn typed_concurrent_metrics() {
        use std::sync::Arc;

        let metrics = Arc::new(MetricsMiddleware::new());
        let handles: Vec<_> = (0..20)
            .map(|i| {
                let m = Arc::clone(&metrics);
                tokio::spawn(async move {
                    let mut ev = if i % 2 == 0 {
                        run_started()
                    } else {
                        error_ev()
                    };
                    m.on_event(&mut ev);
                })
            })
            .collect();

        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(metrics.total(), 20);
        assert_eq!(metrics.counts()["run_started"], 10);
        assert_eq!(metrics.counts()["error"], 10);
    }

    // ── Typed performance ───────────────────────────────────────────

    #[test]
    fn typed_large_chain_performance() {
        let mut chain = SidecarMiddlewareChain::new();
        for _ in 0..50 {
            chain.push(TypedLogging::new());
        }

        let start = std::time::Instant::now();
        for _ in 0..100 {
            let mut ev = run_started();
            chain.process(&mut ev);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs() < 5,
            "100 events through 50 typed middlewares took {elapsed:?}"
        );
    }

    // ── MiddlewareAction equality ───────────────────────────────────

    #[test]
    fn middleware_action_eq() {
        assert_eq!(MiddlewareAction::Continue, MiddlewareAction::Continue);
        assert_eq!(MiddlewareAction::Skip, MiddlewareAction::Skip);
        assert_eq!(
            MiddlewareAction::Error("x".into()),
            MiddlewareAction::Error("x".into())
        );
        assert_ne!(MiddlewareAction::Continue, MiddlewareAction::Skip);
        assert_ne!(
            MiddlewareAction::Error("a".into()),
            MiddlewareAction::Error("b".into())
        );
    }

    #[test]
    fn middleware_action_debug() {
        let a = MiddlewareAction::Continue;
        let dbg = format!("{a:?}");
        assert!(dbg.contains("Continue"));
    }

    #[test]
    fn middleware_action_clone() {
        let a = MiddlewareAction::Error("msg".into());
        let b = a.clone();
        assert_eq!(a, b);
    }
}
