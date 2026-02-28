// SPDX-License-Identifier: MIT OR Apache-2.0
use serde_json::{Value, json};
use sidecar_kit::middleware::{
    EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain,
};

// ── helpers ──────────────────────────────────────────────────────────

fn run_started() -> Value {
    json!({
        "type": "run_started",
        "message": "starting"
    })
}

fn assistant_message() -> Value {
    json!({
        "type": "assistant_message",
        "text": "hello"
    })
}

fn error_event() -> Value {
    json!({
        "type": "error",
        "message": "something went wrong"
    })
}

/// Test middleware that uppercases the `"message"` field, if present.
struct UppercaseMiddleware;

impl EventMiddleware for UppercaseMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let mut out = event.clone();
        if let Some(msg) = out.get("message").and_then(Value::as_str).map(str::to_uppercase) {
            out["message"] = Value::String(msg);
        }
        Some(out)
    }
}

/// Test middleware that unconditionally drops every event.
struct RejectMiddleware;

impl EventMiddleware for RejectMiddleware {
    fn process(&self, _event: &Value) -> Option<Value> {
        None
    }
}

/// Test middleware that appends a `"tag"` field with a given value.
struct TagMiddleware {
    tag: String,
}

impl TagMiddleware {
    fn new(tag: &str) -> Self {
        Self { tag: tag.into() }
    }
}

impl EventMiddleware for TagMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let mut out = event.clone();
        out["tag"] = Value::String(self.tag.clone());
        Some(out)
    }
}

// ── Single middleware transforms events ──────────────────────────────

#[test]
fn single_middleware_transforms_event() {
    let mw = UppercaseMiddleware;
    let event = run_started();
    let result = mw.process(&event).unwrap();
    assert_eq!(result["message"], "STARTING");
    // Type field preserved
    assert_eq!(result["type"], "run_started");
}

#[test]
fn single_middleware_passthrough_preserves_all_fields() {
    let mw = LoggingMiddleware::new();
    let event = json!({"type": "tool_call", "tool_name": "grep", "input": {"q": "foo"}});
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

// ── Chain of middlewares applies in order ────────────────────────────

#[test]
fn chain_applies_middlewares_in_order() {
    let chain = MiddlewareChain::new()
        .with(TagMiddleware::new("first"))
        .with(UppercaseMiddleware);

    let result = chain.process(&run_started()).unwrap();
    // TagMiddleware ran first (added "tag"), then UppercaseMiddleware ran
    // (uppercased "message" and also "tag" has no "message" key concern —
    // but "STARTING" should be the message).
    assert_eq!(result["message"], "STARTING");
    assert_eq!(result["tag"], "first");
}

#[test]
fn chain_order_matters() {
    // First uppercase "starting" -> "STARTING", then tag it.
    let chain_a = MiddlewareChain::new()
        .with(UppercaseMiddleware)
        .with(TagMiddleware::new("after_upper"));

    let result_a = chain_a.process(&run_started()).unwrap();
    assert_eq!(result_a["message"], "STARTING");
    assert_eq!(result_a["tag"], "after_upper");

    // First tag, then uppercase
    let chain_b = MiddlewareChain::new()
        .with(TagMiddleware::new("before_upper"))
        .with(UppercaseMiddleware);

    let result_b = chain_b.process(&run_started()).unwrap();
    assert_eq!(result_b["message"], "STARTING");
    assert_eq!(result_b["tag"], "before_upper");
}

// ── FilterMiddleware drops events ───────────────────────────────────

#[test]
fn filter_include_passes_matching_kind() {
    let filter = FilterMiddleware::include_kinds(&["run_started"]);
    assert!(filter.process(&run_started()).is_some());
}

#[test]
fn filter_include_drops_non_matching_kind() {
    let filter = FilterMiddleware::include_kinds(&["run_started"]);
    assert!(filter.process(&assistant_message()).is_none());
}

#[test]
fn filter_exclude_drops_matching_kind() {
    let filter = FilterMiddleware::exclude_kinds(&["error"]);
    assert!(filter.process(&error_event()).is_none());
}

#[test]
fn filter_exclude_passes_non_matching_kind() {
    let filter = FilterMiddleware::exclude_kinds(&["error"]);
    assert!(filter.process(&run_started()).is_some());
}

#[test]
fn filter_is_case_insensitive() {
    let filter = FilterMiddleware::include_kinds(&["RUN_STARTED"]);
    assert!(filter.process(&run_started()).is_some());
}

#[test]
fn filter_empty_include_drops_everything() {
    let filter = FilterMiddleware::include_kinds(&[]);
    assert!(filter.process(&run_started()).is_none());
    assert!(filter.process(&assistant_message()).is_none());
}

#[test]
fn filter_empty_exclude_passes_everything() {
    let filter = FilterMiddleware::exclude_kinds(&[]);
    assert!(filter.process(&run_started()).is_some());
    assert!(filter.process(&error_event()).is_some());
}

#[test]
fn filter_event_without_type_field() {
    let filter = FilterMiddleware::include_kinds(&["run_started"]);
    let event = json!({"data": 42});
    // No "type" field → type_name is "" → not in include set → dropped
    assert!(filter.process(&event).is_none());

    let exclude = FilterMiddleware::exclude_kinds(&["run_started"]);
    // "" is not in the exclude set → passes
    assert!(exclude.process(&event).is_some());
}

// ── Logging middleware passes through ────────────────────────────────

#[test]
fn logging_middleware_is_transparent() {
    let mw = LoggingMiddleware::new();
    let event = assistant_message();
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

#[test]
fn logging_middleware_default() {
    let mw = LoggingMiddleware;
    let event = run_started();
    let result = mw.process(&event).unwrap();
    assert_eq!(result, event);
}

// ── Empty chain is passthrough ──────────────────────────────────────

#[test]
fn empty_chain_is_passthrough() {
    let chain = MiddlewareChain::new();
    assert!(chain.is_empty());
    let event = run_started();
    let result = chain.process(&event).unwrap();
    assert_eq!(result, event);
}

#[test]
fn empty_chain_default_is_passthrough() {
    let chain = MiddlewareChain::default();
    assert_eq!(chain.len(), 0);
    let event = error_event();
    let result = chain.process(&event).unwrap();
    assert_eq!(result, event);
}

// ── Middleware can reject events ─────────────────────────────────────

#[test]
fn reject_middleware_drops_all_events() {
    let mw = RejectMiddleware;
    assert!(mw.process(&run_started()).is_none());
    assert!(mw.process(&assistant_message()).is_none());
    assert!(mw.process(&error_event()).is_none());
}

// ── Chain short-circuits on None ─────────────────────────────────────

#[test]
fn chain_short_circuits_on_none() {
    // Reject sits in the middle — TagMiddleware after it should never run.
    let chain = MiddlewareChain::new()
        .with(UppercaseMiddleware)
        .with(RejectMiddleware)
        .with(TagMiddleware::new("unreachable"));

    assert!(chain.process(&run_started()).is_none());
}

#[test]
fn chain_short_circuits_with_filter() {
    let chain = MiddlewareChain::new()
        .with(FilterMiddleware::include_kinds(&["error"]))
        .with(TagMiddleware::new("seen"));

    // error passes filter → gets tagged
    let result = chain.process(&error_event()).unwrap();
    assert_eq!(result["tag"], "seen");

    // run_started is filtered out → tag never applied
    assert!(chain.process(&run_started()).is_none());
}

// ── MiddlewareChain builder / len helpers ────────────────────────────

#[test]
fn chain_len_and_push() {
    let mut chain = MiddlewareChain::new();
    assert_eq!(chain.len(), 0);
    assert!(chain.is_empty());

    chain.push(LoggingMiddleware);
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());

    chain.push(UppercaseMiddleware);
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_with_builder() {
    let chain = MiddlewareChain::new()
        .with(LoggingMiddleware)
        .with(UppercaseMiddleware)
        .with(FilterMiddleware::exclude_kinds(&["error"]));

    assert_eq!(chain.len(), 3);
}

// ── Concurrent middleware usage ──────────────────────────────────────

#[tokio::test]
async fn concurrent_middleware_usage_via_arc() {
    use std::sync::Arc;

    let chain = Arc::new(
        MiddlewareChain::new()
            .with(FilterMiddleware::exclude_kinds(&["error"]))
            .with(UppercaseMiddleware),
    );

    let mut handles = Vec::new();
    for i in 0..10 {
        let chain = Arc::clone(&chain);
        handles.push(tokio::spawn(async move {
            let event = json!({"type": "run_started", "message": format!("msg-{i}")});
            chain.process(&event)
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        assert_eq!(result["type"], "run_started");
        // message should be uppercased
        let msg = result["message"].as_str().unwrap();
        assert!(msg.starts_with("MSG-"));
    }
}

#[tokio::test]
async fn concurrent_filter_drops_across_tasks() {
    use std::sync::Arc;

    let chain = Arc::new(
        MiddlewareChain::new().with(FilterMiddleware::include_kinds(&["assistant_message"])),
    );

    let mut handles = Vec::new();
    // Half should pass, half should be dropped
    for i in 0..10 {
        let chain = Arc::clone(&chain);
        handles.push(tokio::spawn(async move {
            let event = if i % 2 == 0 {
                json!({"type": "assistant_message", "text": format!("hi-{i}")})
            } else {
                json!({"type": "error", "message": format!("err-{i}")})
            };
            chain.process(&event)
        }));
    }

    let mut passed = 0;
    let mut dropped = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Some(_) => passed += 1,
            None => dropped += 1,
        }
    }
    assert_eq!(passed, 5);
    assert_eq!(dropped, 5);
}

// ── Closure-based middleware via blanket impl alternative ────────────

/// Verify custom middleware impls work with the chain.
#[test]
fn custom_middleware_integrates_with_chain() {
    struct PrefixMiddleware {
        prefix: String,
    }

    impl EventMiddleware for PrefixMiddleware {
        fn process(&self, event: &Value) -> Option<Value> {
            let mut out = event.clone();
            if let Some(text) = out.get("text").and_then(Value::as_str) {
                out["text"] = Value::String(format!("{}{}", self.prefix, text));
            }
            Some(out)
        }
    }

    let chain = MiddlewareChain::new()
        .with(PrefixMiddleware {
            prefix: "[bot] ".into(),
        })
        .with(UppercaseMiddleware);

    let result = chain
        .process(&json!({"type": "assistant_message", "text": "world", "message": "x"}))
        .unwrap();
    assert_eq!(result["text"], "[bot] world");
    assert_eq!(result["message"], "X");
}
