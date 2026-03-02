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

// =========================================================================
// Typed SidecarMiddleware tests
// =========================================================================

mod typed {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;
    use sidecar_kit::typed_middleware::{
        ErrorRecoveryMiddleware, FilterMiddleware as TypedFilter,
        LoggingMiddleware as TypedLogging, MetricsMiddleware, MiddlewareAction,
        RateLimitMiddleware, SidecarMiddleware, SidecarMiddlewareChain,
    };

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
        make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        })
    }

    fn error_event() -> AgentEvent {
        make_event(AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        })
    }

    // ── Chain executes middlewares in order ──────────────────────────

    /// Middleware that tags the event ext with an order marker.
    struct OrderMarker {
        label: String,
    }

    impl SidecarMiddleware for OrderMarker {
        fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
            let ext = event.ext.get_or_insert_with(Default::default);
            let list = ext
                .entry("order".to_string())
                .or_insert_with(|| serde_json::json!([]));
            if let Some(arr) = list.as_array_mut() {
                arr.push(serde_json::json!(self.label));
            }
            MiddlewareAction::Continue
        }
    }

    #[test]
    fn chain_executes_middlewares_in_order() {
        let chain = SidecarMiddlewareChain::new()
            .with(OrderMarker {
                label: "first".into(),
            })
            .with(OrderMarker {
                label: "second".into(),
            })
            .with(OrderMarker {
                label: "third".into(),
            });

        let mut event = run_started();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Continue);
        let order = event.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order, vec!["first", "second", "third"]);
    }

    // ── Skip action prevents further processing ─────────────────────

    struct SkipAll;
    impl SidecarMiddleware for SkipAll {
        fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
            MiddlewareAction::Skip
        }
    }

    #[test]
    fn skip_action_prevents_further_processing() {
        let chain = SidecarMiddlewareChain::new()
            .with(OrderMarker {
                label: "first".into(),
            })
            .with(SkipAll)
            .with(OrderMarker {
                label: "unreachable".into(),
            });

        let mut event = run_started();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Skip);
        let order = event.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(order.len(), 1);
        assert_eq!(order[0], "first");
    }

    // ── Error action generates error ────────────────────────────────

    #[test]
    fn error_action_short_circuits_chain() {
        struct ErrorMw;
        impl SidecarMiddleware for ErrorMw {
            fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
                MiddlewareAction::Error("failed".into())
            }
        }

        let chain = SidecarMiddlewareChain::new()
            .with(ErrorMw)
            .with(OrderMarker {
                label: "unreachable".into(),
            });

        let mut event = run_started();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Error("failed".into()));
        assert!(event.ext.is_none());
    }

    // ── Logging middleware emits tracing events ─────────────────────

    #[test]
    fn logging_middleware_emits_tracing_events() {
        let mw = TypedLogging::new();
        let mut event = assistant_msg();
        let action = mw.on_event(&mut event);
        assert_eq!(action, MiddlewareAction::Continue);
        // Event should be unchanged (logging is non-mutating).
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
    }

    // ── Rate limiter limits throughput ───────────────────────────────

    #[test]
    fn rate_limiter_limits_throughput() {
        let limiter = RateLimitMiddleware::new(3);
        let mut passed = 0;
        let mut skipped = 0;
        for _ in 0..10 {
            let mut event = run_started();
            match limiter.on_event(&mut event) {
                MiddlewareAction::Continue => passed += 1,
                MiddlewareAction::Skip => skipped += 1,
                _ => panic!("unexpected action"),
            }
        }
        assert_eq!(passed, 3);
        assert_eq!(skipped, 7);
    }

    // ── Filter drops matching events ────────────────────────────────

    #[test]
    fn filter_drops_matching_events() {
        let filter = TypedFilter::new(|event: &AgentEvent| {
            matches!(event.kind, AgentEventKind::Error { .. })
        });

        let mut ev = error_event();
        assert_eq!(filter.on_event(&mut ev), MiddlewareAction::Skip);

        let mut ev2 = run_started();
        assert_eq!(filter.on_event(&mut ev2), MiddlewareAction::Continue);
    }

    // ── Error recovery catches panics ───────────────────────────────

    struct PanickingMiddleware;
    impl SidecarMiddleware for PanickingMiddleware {
        fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
            panic!("oh no");
        }
    }

    #[test]
    fn error_recovery_catches_panics() {
        let recovery = ErrorRecoveryMiddleware::wrap(PanickingMiddleware);
        let mut event = run_started();
        let action = recovery.on_event(&mut event);
        assert_eq!(action, MiddlewareAction::Error("oh no".into()));
    }

    // ── Empty chain passes everything through ───────────────────────

    #[test]
    fn empty_chain_passes_everything_through() {
        let chain = SidecarMiddlewareChain::new();
        assert!(chain.is_empty());
        let mut event = assistant_msg();
        let action = chain.process(&mut event);
        assert_eq!(action, MiddlewareAction::Continue);
    }

    // ── Multiple chains compose correctly ───────────────────────────

    /// Adapter that runs a sub-chain as a single middleware.
    struct SubChain(SidecarMiddlewareChain);
    impl SidecarMiddleware for SubChain {
        fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
            self.0.process(event)
        }
    }

    #[test]
    fn multiple_chains_compose_correctly() {
        let inner = SidecarMiddlewareChain::new()
            .with(OrderMarker {
                label: "inner_1".into(),
            })
            .with(OrderMarker {
                label: "inner_2".into(),
            });

        let outer = SidecarMiddlewareChain::new()
            .with(OrderMarker {
                label: "outer_before".into(),
            })
            .with(SubChain(inner))
            .with(OrderMarker {
                label: "outer_after".into(),
            });

        let mut event = run_started();
        let action = outer.process(&mut event);
        assert_eq!(action, MiddlewareAction::Continue);
        let order = event.ext.unwrap()["order"].as_array().unwrap().clone();
        assert_eq!(
            order,
            vec!["outer_before", "inner_1", "inner_2", "outer_after"]
        );
    }

    // ── Metrics middleware counts events by type ─────────────────────

    #[test]
    fn metrics_counts_events_by_type() {
        let metrics = MetricsMiddleware::new();
        let mut e1 = run_started();
        let mut e2 = run_started();
        let mut e3 = error_event();
        metrics.on_event(&mut e1);
        metrics.on_event(&mut e2);
        metrics.on_event(&mut e3);

        let counts = metrics.counts();
        assert_eq!(counts["run_started"], 2);
        assert_eq!(counts["error"], 1);
        assert_eq!(metrics.total(), 3);
        assert_eq!(metrics.timings().len(), 3);
    }
}
