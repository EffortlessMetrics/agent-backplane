// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the event bus.

use abp_core::{AgentEvent, AgentEventKind};
use abp_runtime::bus::{EventBus, EventBusStats, FilteredSubscription};
use chrono::Utc;
use std::sync::Arc;

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

fn run_completed() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

fn tool_call() -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

// ── Basic publish / subscribe ──────────────────────────────────────────

#[tokio::test]
async fn publish_and_receive_single_event() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    bus.publish(run_started());

    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn multiple_subscribers_receive_same_event() {
    let bus = EventBus::new();
    let mut sub1 = bus.subscribe();
    let mut sub2 = bus.subscribe();

    bus.publish(run_started());

    let e1 = sub1.recv().await.unwrap();
    let e2 = sub2.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(e2.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn publish_without_subscribers_does_not_panic() {
    let bus = EventBus::new();
    bus.publish(run_started()); // should silently drop
}

// ── Subscriber count ───────────────────────────────────────────────────

#[tokio::test]
async fn subscriber_count_tracks_subscriptions() {
    let bus = EventBus::new();
    assert_eq!(bus.subscriber_count(), 0);

    let s1 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 1);

    let s2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    drop(s1);
    assert_eq!(bus.subscriber_count(), 1);

    drop(s2);
    assert_eq!(bus.subscriber_count(), 0);
}

// ── try_recv ───────────────────────────────────────────────────────────

#[tokio::test]
async fn try_recv_returns_none_when_empty() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    assert!(sub.try_recv().is_none());
}

#[tokio::test]
async fn try_recv_returns_event_when_available() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    bus.publish(run_started());
    assert!(sub.try_recv().is_some());
}

// ── Channel closed ─────────────────────────────────────────────────────

#[tokio::test]
async fn recv_returns_none_after_bus_dropped() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    drop(bus);

    assert!(sub.recv().await.is_none());
}

// ── Statistics ─────────────────────────────────────────────────────────

#[tokio::test]
async fn stats_track_publishes_and_drops() {
    let bus = EventBus::new();

    // Publish with no subscribers → dropped.
    bus.publish(run_started());
    let s = bus.stats();
    assert_eq!(s.total_published, 1);
    assert_eq!(s.dropped_events, 1);
    assert_eq!(s.active_subscribers, 0);

    // Now subscribe and publish again.
    let _sub = bus.subscribe();
    bus.publish(run_started());
    let s = bus.stats();
    assert_eq!(s.total_published, 2);
    assert_eq!(s.dropped_events, 1); // still only 1 from before
    assert_eq!(s.active_subscribers, 1);
}

#[tokio::test]
async fn stats_snapshot_is_eq() {
    let a = EventBusStats {
        total_published: 5,
        active_subscribers: 2,
        dropped_events: 1,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

// ── Filtered subscription ──────────────────────────────────────────────

#[tokio::test]
async fn filtered_subscription_yields_only_matching_events() {
    let bus = EventBus::new();
    let sub = bus.subscribe();

    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::ToolCall { .. })),
    );

    bus.publish(run_started());
    bus.publish(tool_call());
    bus.publish(run_completed());

    let ev = filtered.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[tokio::test]
async fn filtered_subscription_try_recv() {
    let bus = EventBus::new();
    let sub = bus.subscribe();

    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::RunCompleted { .. })),
    );

    bus.publish(run_started());
    bus.publish(run_completed());

    let ev = filtered.try_recv().unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

// ── Concurrent subscribers ─────────────────────────────────────────────

#[tokio::test]
async fn concurrent_subscribers_all_receive() {
    let bus = Arc::new(EventBus::new());
    let mut handles = Vec::new();

    for _ in 0..4 {
        let mut sub = bus.subscribe();
        handles.push(tokio::spawn(async move {
            let ev = sub.recv().await.unwrap();
            assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
        }));
    }

    tokio::task::yield_now().await;
    bus.publish(run_started());

    for h in handles {
        h.await.unwrap();
    }
}

// ── Default impl ───────────────────────────────────────────────────────

#[tokio::test]
async fn default_creates_working_bus() {
    let bus = EventBus::default();
    let mut sub = bus.subscribe();
    bus.publish(run_started());
    assert!(sub.recv().await.is_some());
}
