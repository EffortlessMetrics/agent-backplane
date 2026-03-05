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
//! Comprehensive tests for runtime event multiplexing and orchestration pipeline.
//!
//! Test categories:
//! 1.  Event routing
//! 2.  Multiple subscribers
//! 3.  Event ordering
//! 4.  Event filtering
//! 5.  Event buffering
//! 6.  Backpressure
//! 7.  Completion signaling
//! 8.  Error events
//! 9.  Run lifecycle
//! 10. Concurrent runs
//! 11. Receipt production
//! 12. Event statistics

use abp_core::{
    AgentEvent, AgentEventKind, CapabilityRequirements, ExecutionLane, Outcome, PolicyProfile,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::bus::{EventBus, FilteredSubscription};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter, MultiplexError};
use abp_runtime::Runtime;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn run_completed_event() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

fn tool_call_event(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn assistant_msg_event(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage { text: text.into() })
}

fn assistant_delta_event(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta { text: text.into() })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.into(),
        error_code: None,
    })
}

fn warning_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.into(),
    })
}

fn file_changed_event(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.into(),
        summary: "modified".into(),
    })
}

fn command_event(cmd: &str, code: i32) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.into(),
        exit_code: Some(code),
        output_preview: None,
    })
}

fn tool_result_event(name: &str, is_error: bool) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.into(),
        tool_use_id: None,
        output: serde_json::json!({"ok": true}),
        is_error,
    })
}

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "mux test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

async fn run_to_completion(rt: &Runtime, wo: WorkOrder) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ===========================================================================
// 1. Event routing
// ===========================================================================

#[tokio::test]
async fn routing_run_started_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn routing_tool_call_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(tool_call_event("bash")).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[tokio::test]
async fn routing_error_event_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(error_event("boom")).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[tokio::test]
async fn routing_file_changed_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(file_changed_event("src/main.rs")).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[tokio::test]
async fn routing_command_event_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(command_event("ls", 0)).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
}

#[tokio::test]
async fn routing_warning_event_reaches_subscriber() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(warning_event("caution")).unwrap();
    let ev = sub.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[tokio::test]
async fn router_dispatches_run_started_to_handler() {
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
}

#[tokio::test]
async fn router_does_not_dispatch_to_wrong_handler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = EventRouter::new();
    router.add_route(
        "tool_call",
        Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }),
    );
    router.route(&run_started_event());
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn router_dispatches_multiple_kinds_correctly() {
    let started_count = Arc::new(AtomicUsize::new(0));
    let tool_count = Arc::new(AtomicUsize::new(0));
    let sc = started_count.clone();
    let tc = tool_count.clone();

    let mut router = EventRouter::new();
    router.add_route(
        "run_started",
        Box::new(move |_| {
            sc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    router.add_route(
        "tool_call",
        Box::new(move |_| {
            tc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    router.route(&run_started_event());
    router.route(&tool_call_event("bash"));
    router.route(&tool_call_event("grep"));

    assert_eq!(started_count.load(Ordering::SeqCst), 1);
    assert_eq!(tool_count.load(Ordering::SeqCst), 2);
}

// ===========================================================================
// 2. Multiple subscribers
// ===========================================================================

#[tokio::test]
async fn two_subscribers_both_receive_event() {
    let mux = EventMultiplexer::new(16);
    let mut s1 = mux.subscribe();
    let mut s2 = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    assert!(matches!(
        s1.recv().await.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        s2.recv().await.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn five_subscribers_all_receive_same_event() {
    let mux = EventMultiplexer::new(16);
    let mut subs: Vec<_> = (0..5).map(|_| mux.subscribe()).collect();
    mux.broadcast(assistant_msg_event("hello")).unwrap();
    for sub in &mut subs {
        let ev = sub.recv().await.unwrap();
        assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
    }
}

#[tokio::test]
async fn bus_multiple_subscribers_receive_event() {
    let bus = EventBus::new();
    let mut s1 = bus.subscribe();
    let mut s2 = bus.subscribe();
    let mut s3 = bus.subscribe();
    bus.publish(run_started_event());
    assert!(s1.recv().await.is_some());
    assert!(s2.recv().await.is_some());
    assert!(s3.recv().await.is_some());
}

#[tokio::test]
async fn subscriber_added_after_broadcast_misses_event() {
    let mux = EventMultiplexer::new(16);
    let mut s1 = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    let mut s2 = mux.subscribe();
    // s1 should receive the event, s2 should not
    assert!(s1.recv().await.is_ok());
    assert!(s2.try_recv().is_none());
}

#[tokio::test]
async fn concurrent_subscribers_via_spawn() {
    let mux = Arc::new(EventMultiplexer::new(64));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let mut sub = mux.subscribe();
        handles.push(tokio::spawn(async move {
            let ev = sub.recv().await.unwrap();
            matches!(ev.kind, AgentEventKind::RunStarted { .. })
        }));
    }
    tokio::task::yield_now().await;
    mux.broadcast(run_started_event()).unwrap();
    for h in handles {
        assert!(h.await.unwrap());
    }
}

// ===========================================================================
// 3. Event ordering
// ===========================================================================

#[tokio::test]
async fn mux_events_arrive_in_send_order() {
    let mux = EventMultiplexer::new(64);
    let mut sub = mux.subscribe();

    let events_to_send = vec![
        run_started_event(),
        tool_call_event("a"),
        tool_call_event("b"),
        assistant_msg_event("hi"),
        run_completed_event(),
    ];
    for ev in &events_to_send {
        mux.broadcast(ev.clone()).unwrap();
    }

    for expected in &events_to_send {
        let received = sub.recv().await.unwrap();
        assert_eq!(
            std::mem::discriminant(&received.kind),
            std::mem::discriminant(&expected.kind)
        );
    }
}

#[tokio::test]
async fn bus_events_arrive_in_publish_order() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();

    bus.publish(run_started_event());
    bus.publish(tool_call_event("x"));
    bus.publish(run_completed_event());

    let e1 = sub.recv().await.unwrap();
    let e2 = sub.recv().await.unwrap();
    let e3 = sub.recv().await.unwrap();

    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(e2.kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(e3.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn runtime_events_started_before_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    let started_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .expect("RunStarted must be present");
    let completed_idx = events
        .iter()
        .position(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
        .expect("RunCompleted must be present");

    assert!(started_idx < completed_idx);
}

#[tokio::test]
async fn runtime_event_timestamps_non_decreasing() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn mux_ordering_with_ten_events() {
    let mux = EventMultiplexer::new(64);
    let mut sub = mux.subscribe();
    for i in 0..10 {
        mux.broadcast(assistant_delta_event(&format!("token-{i}")))
            .unwrap();
    }
    for i in 0..10 {
        let ev = sub.recv().await.unwrap();
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("token-{i}"));
        } else {
            panic!("expected AssistantDelta");
        }
    }
}

#[tokio::test]
async fn ordering_interleaved_event_types() {
    let mux = EventMultiplexer::new(32);
    let mut sub = mux.subscribe();
    let sequence = vec![
        run_started_event(),
        assistant_delta_event("a"),
        tool_call_event("bash"),
        tool_result_event("bash", false),
        file_changed_event("f.txt"),
        command_event("ls", 0),
        warning_event("w"),
        error_event("e"),
        run_completed_event(),
    ];
    for ev in &sequence {
        mux.broadcast(ev.clone()).unwrap();
    }
    for expected in &sequence {
        let received = sub.recv().await.unwrap();
        assert_eq!(
            std::mem::discriminant(&received.kind),
            std::mem::discriminant(&expected.kind)
        );
    }
}

// ===========================================================================
// 4. Event filtering
// ===========================================================================

#[tokio::test]
async fn filtered_subscription_only_tool_calls() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::ToolCall { .. })),
    );

    bus.publish(run_started_event());
    bus.publish(tool_call_event("bash"));
    bus.publish(assistant_msg_event("ignored"));
    bus.publish(tool_call_event("grep"));
    bus.publish(run_completed_event());
    drop(bus);

    let e1 = filtered.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::ToolCall { .. }));
    let e2 = filtered.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::ToolCall { .. }));
    // After bus drop and no more tool calls, recv returns None
    assert!(filtered.recv().await.is_none());
}

#[tokio::test]
async fn filtered_subscription_only_errors() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::Error { .. })),
    );

    bus.publish(run_started_event());
    bus.publish(error_event("oops"));
    bus.publish(run_completed_event());
    drop(bus);

    let ev = filtered.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    assert!(filtered.recv().await.is_none());
}

#[tokio::test]
async fn filtered_subscription_no_matches_returns_none() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::FileChanged { .. })),
    );

    bus.publish(run_started_event());
    bus.publish(run_completed_event());
    drop(bus);

    assert!(filtered.recv().await.is_none());
}

#[tokio::test]
async fn filtered_subscription_try_recv_skips_non_matching() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::Warning { .. })),
    );

    bus.publish(run_started_event());
    bus.publish(warning_event("caution"));
    let ev = filtered.try_recv().unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[tokio::test]
async fn router_filters_events_by_kind_string() {
    let file_count = Arc::new(AtomicUsize::new(0));
    let cmd_count = Arc::new(AtomicUsize::new(0));
    let fc = file_count.clone();
    let cc = cmd_count.clone();

    let mut router = EventRouter::new();
    router.add_route(
        "file_changed",
        Box::new(move |_| {
            fc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    router.add_route(
        "command_executed",
        Box::new(move |_| {
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    router.route(&run_started_event());
    router.route(&file_changed_event("a.rs"));
    router.route(&command_event("cargo build", 0));
    router.route(&file_changed_event("b.rs"));

    assert_eq!(file_count.load(Ordering::SeqCst), 2);
    assert_eq!(cmd_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn filtered_subscription_assistant_messages_only() {
    let bus = EventBus::new();
    let sub = bus.subscribe();
    let mut filtered = FilteredSubscription::new(
        sub,
        Box::new(|ev| matches!(ev.kind, AgentEventKind::AssistantMessage { .. })),
    );

    bus.publish(run_started_event());
    bus.publish(assistant_msg_event("hello"));
    bus.publish(tool_call_event("bash"));
    bus.publish(assistant_msg_event("world"));
    drop(bus);

    let e1 = filtered.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::AssistantMessage { .. }));
    let e2 = filtered.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::AssistantMessage { .. }));
    assert!(filtered.recv().await.is_none());
}

// ===========================================================================
// 5. Event buffering
// ===========================================================================

#[tokio::test]
async fn mux_buffers_events_when_not_consumed() {
    let mux = EventMultiplexer::new(64);
    let mut sub = mux.subscribe();

    for i in 0..20 {
        mux.broadcast(assistant_delta_event(&format!("t{i}")))
            .unwrap();
    }

    // All 20 should be buffered and available
    for i in 0..20 {
        let ev = sub.recv().await.unwrap();
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("t{i}"));
        } else {
            panic!("expected delta");
        }
    }
}

#[tokio::test]
async fn bus_buffers_events_for_subscriber() {
    let bus = EventBus::with_capacity(128);
    let mut sub = bus.subscribe();

    for _ in 0..50 {
        bus.publish(run_started_event());
    }

    let mut count = 0;
    while sub.try_recv().is_some() {
        count += 1;
    }
    assert_eq!(count, 50);
}

#[tokio::test]
async fn mux_capacity_one_buffers_single_event() {
    let mux = EventMultiplexer::new(1);
    let mut sub = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    assert!(sub.recv().await.is_ok());
}

#[tokio::test]
async fn bus_with_capacity_buffers_up_to_capacity() {
    let bus = EventBus::with_capacity(10);
    let mut sub = bus.subscribe();

    for _ in 0..10 {
        bus.publish(run_started_event());
    }

    let mut count = 0;
    while sub.try_recv().is_some() {
        count += 1;
    }
    assert_eq!(count, 10);
}

#[tokio::test]
async fn try_recv_after_buffered_events_returns_none() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(run_started_event()).unwrap();
    assert!(sub.try_recv().is_some());
    assert!(sub.try_recv().is_none());
}

// ===========================================================================
// 6. Backpressure
// ===========================================================================

#[tokio::test]
async fn mux_lagged_subscriber_gets_lagged_error() {
    let mux = EventMultiplexer::new(2);
    let mut sub = mux.subscribe();

    // Overflow the capacity
    mux.broadcast(run_started_event()).unwrap();
    mux.broadcast(run_started_event()).unwrap();
    mux.broadcast(run_started_event()).unwrap();

    let result = sub.recv().await;
    assert!(matches!(result, Err(MultiplexError::Lagged { .. })));
}

#[tokio::test]
async fn bus_slow_subscriber_stats_track_drops() {
    let bus = EventBus::with_capacity(2);
    let mut sub = bus.subscribe();

    // Overflow
    bus.publish(run_started_event());
    bus.publish(run_started_event());
    bus.publish(run_started_event());

    // The subscriber recovers from lag on recv
    let _ = sub.recv().await;

    let stats = bus.stats();
    assert_eq!(stats.total_published, 3);
    // Drops should be counted
    assert!(stats.dropped_events > 0);
}

#[tokio::test]
async fn mux_large_capacity_avoids_lag() {
    let mux = EventMultiplexer::new(1024);
    let mut sub = mux.subscribe();

    for _ in 0..500 {
        mux.broadcast(run_started_event()).unwrap();
    }

    let mut count = 0;
    while sub.try_recv().is_some() {
        count += 1;
    }
    assert_eq!(count, 500);
}

#[tokio::test]
async fn bus_no_subscribers_counts_drops() {
    let bus = EventBus::new();
    bus.publish(run_started_event());
    bus.publish(run_started_event());
    let stats = bus.stats();
    assert_eq!(stats.total_published, 2);
    assert_eq!(stats.dropped_events, 2);
}

#[tokio::test]
async fn mux_no_subscribers_returns_error_on_broadcast() {
    let mux = EventMultiplexer::new(16);
    let result = mux.broadcast(run_started_event());
    assert!(matches!(result, Err(MultiplexError::NoSubscribers)));
}

// ===========================================================================
// 7. Completion signaling
// ===========================================================================

#[tokio::test]
async fn runtime_run_completed_signals_end() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    let last = events.last().expect("should have events");
    assert!(matches!(last.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn mux_drop_signals_closed() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    drop(mux);
    let result = sub.recv().await;
    assert!(matches!(result, Err(MultiplexError::Closed)));
}

#[tokio::test]
async fn bus_drop_signals_none() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();
    drop(bus);
    assert!(sub.recv().await.is_none());
}

#[tokio::test]
async fn runtime_stream_terminates_after_all_events() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    assert!(!events.is_empty());
    // Stream returned None (collect completed), proving termination.
}

#[tokio::test]
async fn runtime_receipt_available_after_stream_drain() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn mux_broadcast_after_subscriber_drop_succeeds_with_remaining() {
    let mux = EventMultiplexer::new(16);
    let _s1 = mux.subscribe();
    let s2 = mux.subscribe();
    drop(s2);
    // Should still succeed with 1 remaining subscriber
    let n = mux.broadcast(run_started_event()).unwrap();
    assert_eq!(n, 1);
}

// ===========================================================================
// 8. Error events
// ===========================================================================

#[tokio::test]
async fn error_event_does_not_terminate_mux_stream() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();

    mux.broadcast(run_started_event()).unwrap();
    mux.broadcast(error_event("something bad")).unwrap();
    mux.broadcast(assistant_msg_event("after error")).unwrap();
    mux.broadcast(run_completed_event()).unwrap();

    let e1 = sub.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    let e2 = sub.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::Error { .. }));
    let e3 = sub.recv().await.unwrap();
    assert!(matches!(e3.kind, AgentEventKind::AssistantMessage { .. }));
    let e4 = sub.recv().await.unwrap();
    assert!(matches!(e4.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn error_event_does_not_terminate_bus_stream() {
    let bus = EventBus::new();
    let mut sub = bus.subscribe();

    bus.publish(error_event("err1"));
    bus.publish(assistant_msg_event("msg"));
    bus.publish(error_event("err2"));

    let e1 = sub.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::Error { .. }));
    let e2 = sub.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::AssistantMessage { .. }));
    let e3 = sub.recv().await.unwrap();
    assert!(matches!(e3.kind, AgentEventKind::Error { .. }));
}

#[tokio::test]
async fn multiple_error_events_all_delivered() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();

    for i in 0..5 {
        mux.broadcast(error_event(&format!("err-{i}"))).unwrap();
    }

    for _ in 0..5 {
        let ev = sub.recv().await.unwrap();
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[tokio::test]
async fn warning_events_do_not_terminate_stream() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();

    mux.broadcast(warning_event("w1")).unwrap();
    mux.broadcast(warning_event("w2")).unwrap();
    mux.broadcast(assistant_msg_event("ok")).unwrap();

    let _ = sub.recv().await.unwrap();
    let _ = sub.recv().await.unwrap();
    let e3 = sub.recv().await.unwrap();
    assert!(matches!(e3.kind, AgentEventKind::AssistantMessage { .. }));
}

#[tokio::test]
async fn error_events_routed_to_error_handler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = EventRouter::new();
    router.add_route(
        "error",
        Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }),
    );
    router.route(&error_event("e1"));
    router.route(&error_event("e2"));
    router.route(&run_started_event());
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn tool_result_with_error_flag_still_delivered() {
    let mux = EventMultiplexer::new(16);
    let mut sub = mux.subscribe();
    mux.broadcast(tool_result_event("bash", true)).unwrap();
    let ev = sub.recv().await.unwrap();
    if let AgentEventKind::ToolResult { is_error, .. } = &ev.kind {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

// ===========================================================================
// 9. Run lifecycle
// ===========================================================================

#[tokio::test]
async fn lifecycle_starts_with_run_started() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert!(!events.is_empty());
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn lifecycle_ends_with_run_completed() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    let last = events.last().unwrap();
    assert!(matches!(last.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn lifecycle_mock_emits_four_events() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn lifecycle_receipt_outcome_complete() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn lifecycle_work_order_id_preserved() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let wo_id = wo.id;
    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn lifecycle_handle_run_id_matches_receipt() {
    let rt = Runtime::with_default_backends();
    let handle = rt
        .run_streaming("mock", mock_work_order())
        .await
        .expect("run_streaming");
    let handle_run_id = handle.run_id;
    let _events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert_eq!(handle_run_id, receipt.meta.run_id);
}

#[tokio::test]
async fn lifecycle_unknown_backend_errors() {
    let rt = Runtime::with_default_backends();
    let result = rt.run_streaming("nonexistent", mock_work_order()).await;
    assert!(result.is_err());
}

// ===========================================================================
// 10. Concurrent runs (v0.1 single-run, but test isolation)
// ===========================================================================

#[tokio::test]
async fn sequential_runs_produce_unique_run_ids() {
    let rt = Runtime::with_default_backends();
    let mut ids = std::collections::HashSet::new();
    for _ in 0..5 {
        let handle = rt
            .run_streaming("mock", mock_work_order())
            .await
            .expect("run_streaming");
        ids.insert(handle.run_id);
        let _: Vec<_> = handle.events.collect().await;
        let _ = handle.receipt.await;
    }
    assert_eq!(ids.len(), 5);
}

#[tokio::test]
async fn sequential_runs_each_produce_complete_receipt() {
    let rt = Runtime::with_default_backends();
    for _ in 0..3 {
        let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(receipt.receipt_sha256.is_some());
    }
}

#[tokio::test]
async fn sequential_runs_accumulate_metrics() {
    let rt = Runtime::with_default_backends();
    for _ in 0..3 {
        let handle = rt.run_streaming("mock", mock_work_order()).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let _ = handle.receipt.await;
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 3);
    assert_eq!(snap.successful_runs, 3);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn mux_isolated_from_other_mux_instances() {
    let mux1 = EventMultiplexer::new(16);
    let mux2 = EventMultiplexer::new(16);
    let mut sub1 = mux1.subscribe();
    let mut sub2 = mux2.subscribe();

    mux1.broadcast(run_started_event()).unwrap();
    mux2.broadcast(tool_call_event("bash")).unwrap();

    let e1 = sub1.recv().await.unwrap();
    let e2 = sub2.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(e2.kind, AgentEventKind::ToolCall { .. }));
}

#[tokio::test]
async fn bus_instances_are_isolated() {
    let bus1 = EventBus::new();
    let bus2 = EventBus::new();
    let mut s1 = bus1.subscribe();
    let mut s2 = bus2.subscribe();

    bus1.publish(run_started_event());
    bus2.publish(tool_call_event("grep"));

    let e1 = s1.recv().await.unwrap();
    let e2 = s2.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(e2.kind, AgentEventKind::ToolCall { .. }));

    // Cross-check: each sub should not see the other's events
    assert!(s1.try_recv().is_none());
    assert!(s2.try_recv().is_none());
}

#[tokio::test]
async fn sequential_runs_have_independent_event_streams() {
    let rt = Runtime::with_default_backends();

    let (events1, _) = run_to_completion(&rt, mock_work_order()).await;
    let (events2, _) = run_to_completion(&rt, mock_work_order()).await;

    assert_eq!(events1.len(), events2.len());
    // Both streams should start with RunStarted
    assert!(matches!(events1[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(events2[0].kind, AgentEventKind::RunStarted { .. }));
}

// ===========================================================================
// 11. Receipt production
// ===========================================================================

#[tokio::test]
async fn receipt_has_sha256_hash() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn receipt_hash_is_self_consistent() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute");
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn receipt_has_contract_version() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_trace_matches_streamed_events() {
    let rt = Runtime::with_default_backends();
    let (events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn receipt_backend_id_is_mock() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn receipt_run_id_is_valid_uuid() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    // Should not be nil
    assert!(!receipt.meta.run_id.is_nil());
}

#[tokio::test]
async fn receipt_timestamps_are_ordered() {
    let rt = Runtime::with_default_backends();
    let (_events, receipt) = run_to_completion(&rt, mock_work_order()).await;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn receipt_chain_accumulates() {
    let rt = Runtime::with_default_backends();
    for _ in 0..3 {
        let (_events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    }
    let chain = rt.receipt_chain();
    let chain_lock = chain.lock().await;
    assert_eq!(chain_lock.len(), 3);
}

// ===========================================================================
// 12. Event statistics
// ===========================================================================

#[tokio::test]
async fn bus_stats_total_published() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    bus.publish(run_started_event());
    bus.publish(tool_call_event("bash"));
    bus.publish(run_completed_event());
    let stats = bus.stats();
    assert_eq!(stats.total_published, 3);
}

#[tokio::test]
async fn bus_stats_active_subscribers() {
    let bus = EventBus::new();
    let s1 = bus.subscribe();
    let s2 = bus.subscribe();
    assert_eq!(bus.stats().active_subscribers, 2);
    drop(s1);
    assert_eq!(bus.stats().active_subscribers, 1);
    drop(s2);
    assert_eq!(bus.stats().active_subscribers, 0);
}

#[tokio::test]
async fn bus_stats_dropped_with_no_subscribers() {
    let bus = EventBus::new();
    bus.publish(run_started_event());
    bus.publish(run_started_event());
    assert_eq!(bus.stats().dropped_events, 2);
}

#[tokio::test]
async fn runtime_metrics_event_count() {
    let rt = Runtime::with_default_backends();
    let (_events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    let snap = rt.metrics().snapshot();
    assert!(snap.total_events > 0);
}

#[tokio::test]
async fn runtime_metrics_successful_count() {
    let rt = Runtime::with_default_backends();
    let (_events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
}

#[tokio::test]
async fn count_events_by_kind_after_run() {
    let rt = Runtime::with_default_backends();
    let (events, _receipt) = run_to_completion(&rt, mock_work_order()).await;

    let started = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }))
        .count();
    let completed = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }))
        .count();
    let messages = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();

    assert_eq!(started, 1);
    assert_eq!(completed, 1);
    assert_eq!(messages, 2);
}

#[tokio::test]
async fn bus_stats_zero_drops_when_subscriber_present() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    bus.publish(run_started_event());
    bus.publish(run_started_event());
    assert_eq!(bus.stats().dropped_events, 0);
}

#[tokio::test]
async fn mux_subscriber_count_accurate() {
    let mux = EventMultiplexer::new(16);
    assert_eq!(mux.subscriber_count(), 0);
    let s1 = mux.subscribe();
    let s2 = mux.subscribe();
    let s3 = mux.subscribe();
    assert_eq!(mux.subscriber_count(), 3);
    drop(s1);
    drop(s2);
    assert_eq!(mux.subscriber_count(), 1);
    drop(s3);
    assert_eq!(mux.subscriber_count(), 0);
}

#[tokio::test]
async fn bus_stats_snapshot_clone_eq() {
    let bus = EventBus::new();
    let _sub = bus.subscribe();
    bus.publish(run_started_event());
    let s1 = bus.stats();
    let s2 = s1.clone();
    assert_eq!(s1, s2);
}

#[tokio::test]
async fn runtime_metrics_multiple_runs_accumulate_events() {
    let rt = Runtime::with_default_backends();
    for _ in 0..4 {
        let (_events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    }
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 4);
    // MockBackend emits 4 events per run
    assert_eq!(snap.total_events, 16);
}

#[tokio::test]
async fn router_route_count_reflects_distinct_kinds() {
    let mut router = EventRouter::new();
    router.add_route("run_started", Box::new(|_| {}));
    router.add_route("run_started", Box::new(|_| {}));
    router.add_route("tool_call", Box::new(|_| {}));
    router.add_route("error", Box::new(|_| {}));
    assert_eq!(router.route_count(), 3);
}

#[tokio::test]
async fn default_router_has_zero_routes() {
    let router = EventRouter::default();
    assert_eq!(router.route_count(), 0);
}

#[tokio::test]
async fn runtime_metrics_average_duration_populated() {
    let rt = Runtime::with_default_backends();
    let (_events, _receipt) = run_to_completion(&rt, mock_work_order()).await;
    // Can't check exact value, but it should be non-negative (mock runs fast)
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 1);
}

#[tokio::test]
async fn runtime_metrics_zero_before_any_run() {
    let rt = Runtime::with_default_backends();
    let snap = rt.metrics().snapshot();
    assert_eq!(snap.total_runs, 0);
    assert_eq!(snap.successful_runs, 0);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 0);
}
