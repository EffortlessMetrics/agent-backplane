// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the event multiplexer and router.

use abp_core::{AgentEvent, AgentEventKind};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter, MultiplexError};
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn run_started_event() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    })
}

fn tool_call_event() -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

// ── Multiplexer tests ──────────────────────────────────────────────────

#[tokio::test]
async fn single_subscriber_receives_event() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();

    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn multiple_subscribers_receive_same_event() {
    let mux = EventMultiplexer::new(16);
    let mut sub1 = mux.subscribe();
    let mut sub2 = mux.subscribe();

    mux.broadcast(run_started_event()).unwrap();

    let e1 = sub1.recv().await.unwrap();
    let e2 = sub2.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(e2.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn no_subscribers_returns_error() {
    let mux = EventMultiplexer::new(16);
    let err = mux.broadcast(run_started_event()).unwrap_err();
    assert!(matches!(err, MultiplexError::NoSubscribers));
}

#[tokio::test]
async fn subscriber_count_tracking() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);

    let sub1 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 1);

    let sub2 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 2);

    drop(sub1);
    assert_eq!(mux.subscriber_count(), 1);

    drop(sub2);
    assert_eq!(mux.subscriber_count(), 0);
}

#[tokio::test]
async fn lagged_subscriber_handling() {
    // Capacity 2 so the third send forces the oldest out.
    let mux = EventMultiplexer::new(2);
    let mut sub = mux.subscribe();

    // Send 3 events; subscriber can only hold 2.
    mux.broadcast(run_started_event()).unwrap();
    mux.broadcast(run_started_event()).unwrap();
    mux.broadcast(run_started_event()).unwrap();

    let result = sub.recv().await;
    assert!(
        matches!(result, Err(MultiplexError::Lagged { .. })),
        "expected Lagged, got {result:?}"
    );
}

#[tokio::test]
async fn closed_channel_returns_closed() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    drop(mux);

    let result = sub.recv().await;
    assert!(matches!(result, Err(MultiplexError::Closed)), "expected Closed, got {result:?}");
}

#[tokio::test]
async fn try_recv_returns_none_when_empty() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    assert!(sub.try_recv().is_none());
}

#[tokio::test]
async fn try_recv_returns_event_when_available() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    assert!(sub.try_recv().is_some());
}

#[tokio::test]
async fn drop_subscriber_decreases_count() {
    let mux = EventMultiplexer::new(16);
    let s1 = mux.subscribe();
    let s2 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 2);
    drop(s1);
    assert_eq!(mux.subscriber_count(), 1);
    drop(s2);
    assert_eq!(mux.subscriber_count(), 0);
}

#[tokio::test]
async fn empty_multiplexer_has_zero_subscribers() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
}

#[tokio::test]
async fn concurrent_subscribers_receive_events() {
    let mux = Arc::new(EventMultiplexer::new(64));
    let mut handles = Vec::new();

    for _ in 0..4 {
        let mut sub = mux.subscribe();
        handles.push(tokio::spawn(async move {
            let ev = sub.recv().await.unwrap();
            assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
        }));
    }

    // Give subscribers time to start waiting.
    tokio::task::yield_now().await;
    mux.broadcast(run_started_event()).unwrap();

    for h in handles {
        h.await.unwrap();
    }
}

// ── EventRouter tests ──────────────────────────────────────────────────

#[tokio::test]
async fn router_dispatches_to_correct_handler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    let mut router = EventRouter::new();
    router.add_route(
        "run_started",
        Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }),
    );

    router.route(&run_started_event());
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Should not fire for a different kind.
    router.route(&tool_call_event());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn router_multiple_handlers_same_kind() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c1 = counter.clone();
    let c2 = counter.clone();

    let mut router = EventRouter::new();
    router.add_route(
        "tool_call",
        Box::new(move |_| {
            c1.fetch_add(1, Ordering::SeqCst);
        }),
    );
    router.add_route(
        "tool_call",
        Box::new(move |_| {
            c2.fetch_add(10, Ordering::SeqCst);
        }),
    );

    router.route(&tool_call_event());
    assert_eq!(counter.load(Ordering::SeqCst), 11);
}

#[tokio::test]
async fn router_no_matching_handlers_does_not_panic() {
    let router = EventRouter::new();
    // Should be a no-op, not a panic.
    router.route(&run_started_event());
}

#[tokio::test]
async fn router_route_count() {
    let mut router = EventRouter::new();
    assert_eq!(router.route_count(), 0);

    router.add_route("run_started", Box::new(|_| {}));
    assert_eq!(router.route_count(), 1);

    // Second handler for same kind doesn't increase route_count.
    router.add_route("run_started", Box::new(|_| {}));
    assert_eq!(router.route_count(), 1);

    router.add_route("tool_call", Box::new(|_| {}));
    assert_eq!(router.route_count(), 2);
}
