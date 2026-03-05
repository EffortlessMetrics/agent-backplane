#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive streaming fidelity tests verifying event ordering, stream
//! completeness, partial content accumulation, tool call lifecycles, error
//! propagation, backpressure, concurrent stream isolation, cancellation,
//! receipt generation, and event metadata correctness across all SDK dialects.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_with_ext(
    kind: AgentEventKind,
    ext: BTreeMap<String, serde_json::Value>,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn simple_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn simple_receipt(trace: Vec<AgentEvent>) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

async fn collect_events(mut rx: mpsc::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    while let Some(ev) = rx.recv().await {
        out.push(ev);
    }
    out
}

fn kind_name(ev: &AgentEvent) -> &'static str {
    match &ev.kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::FileChanged { .. } => "file_changed",
        AgentEventKind::CommandExecuted { .. } => "command_executed",
        AgentEventKind::Warning { .. } => "warning",
        AgentEventKind::Error { .. } => "error",
    }
}

// ===========================================================================
// 1. Event ordering (tests 1–20)
// ===========================================================================

#[tokio::test]
async fn ordering_run_started_first() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[0]), "run_started");
}

#[tokio::test]
async fn ordering_run_completed_last() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(events.last().unwrap()), "run_completed");
}

#[tokio::test]
async fn ordering_deltas_before_message() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..5 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("d{i}"),
        }))
        .await
        .unwrap();
    }
    tx.send(make_event(AgentEventKind::AssistantMessage {
        text: "full".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let msg_idx = events
        .iter()
        .position(|e| kind_name(e) == "assistant_message")
        .unwrap();
    for event in events.iter().take(5) {
        assert_eq!(kind_name(event), "assistant_delta");
    }
    assert_eq!(msg_idx, 5);
}

#[tokio::test]
async fn ordering_tool_call_before_tool_result() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("t1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "a.rs"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("t1".into()),
        output: json!("content"),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[0]), "tool_call");
    assert_eq!(kind_name(&events[1]), "tool_result");
}

#[tokio::test]
async fn ordering_full_lifecycle_sequence() {
    let (tx, rx) = mpsc::channel(32);
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "thinking...".into(),
        },
        AgentEventKind::AssistantDelta {
            text: " about".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "Bash".into(),
            tool_use_id: Some("tc1".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "Bash".into(),
            tool_use_id: Some("tc1".into()),
            output: json!("file.rs"),
            is_error: false,
        },
        AgentEventKind::AssistantMessage {
            text: "done reasoning".into(),
        },
        AgentEventKind::RunCompleted {
            message: "ok".into(),
        },
    ];
    for k in &kinds {
        tx.send(make_event(k.clone())).await.unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let names: Vec<_> = events.iter().map(kind_name).collect();
    assert_eq!(
        names,
        vec![
            "run_started",
            "assistant_delta",
            "assistant_delta",
            "tool_call",
            "tool_result",
            "assistant_message",
            "run_completed",
        ]
    );
}

#[tokio::test]
async fn ordering_preserves_insertion_order_for_n_events() {
    let (tx, rx) = mpsc::channel(128);
    for i in 0..100 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    for (i, ev) in events.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &i.to_string());
        }
    }
}

#[tokio::test]
async fn ordering_multiple_tool_calls_interleaved() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..3 {
        let id = format!("tc-{i}");
        tx.send(make_event(AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some(id.clone()),
            parent_tool_use_id: None,
            input: json!({"i": i}),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some(id),
            output: json!(i),
            is_error: false,
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 6);
    for chunk in events.chunks(2) {
        assert_eq!(kind_name(&chunk[0]), "tool_call");
        assert_eq!(kind_name(&chunk[1]), "tool_result");
    }
}

#[tokio::test]
async fn ordering_warning_events_preserved() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Warning {
        message: "w1".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Warning {
        message: "w2".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
    if let AgentEventKind::Warning { message } = &events[0].kind {
        assert_eq!(message, "w1");
    }
    if let AgentEventKind::Warning { message } = &events[1].kind {
        assert_eq!(message, "w2");
    }
}

#[tokio::test]
async fn ordering_error_then_completed() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::RunCompleted {
        message: "finished with error".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[0]), "error");
    assert_eq!(kind_name(&events[1]), "run_completed");
}

#[tokio::test]
async fn ordering_file_changed_after_tool() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Write".into(),
        tool_use_id: Some("w1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "f.rs", "content": "fn main() {}"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Write".into(),
        tool_use_id: Some("w1".into()),
        output: json!("ok"),
        is_error: false,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::FileChanged {
        path: "f.rs".into(),
        summary: "created".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[2]), "file_changed");
}

#[tokio::test]
async fn ordering_command_executed_event() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[0]), "command_executed");
}

#[tokio::test]
async fn ordering_empty_stream() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let events = collect_events(rx).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn ordering_single_event_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "only".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn ordering_nested_tool_calls() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Orchestrator".into(),
        tool_use_id: Some("parent".into()),
        parent_tool_use_id: None,
        input: json!({}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("child".into()),
        parent_tool_use_id: Some("parent".into()),
        input: json!({"path": "x.rs"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("child".into()),
        output: json!("data"),
        is_error: false,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Orchestrator".into(),
        tool_use_id: Some("parent".into()),
        output: json!("done"),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 4);
    assert_eq!(kind_name(&events[0]), "tool_call");
    assert_eq!(kind_name(&events[1]), "tool_call");
    assert_eq!(kind_name(&events[2]), "tool_result");
    assert_eq!(kind_name(&events[3]), "tool_result");
}

#[tokio::test]
async fn ordering_delta_burst_preserves_sequence() {
    let (tx, rx) = mpsc::channel(256);
    let expected: Vec<String> = (0..200).map(|i| format!("tok{i}")).collect();
    for tok in &expected {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: tok.clone(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let actual: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn ordering_mixed_warnings_and_deltas() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Warning {
        message: "w".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let names: Vec<_> = events.iter().map(kind_name).collect();
    assert_eq!(names, vec!["assistant_delta", "warning", "assistant_delta"]);
}

#[tokio::test]
async fn ordering_back_to_back_run_started() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
    assert_eq!(kind_name(&events[0]), "run_started");
    assert_eq!(kind_name(&events[1]), "run_started");
}

#[tokio::test]
async fn ordering_timestamps_non_decreasing() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..20 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    for window in events.windows(2) {
        assert!(window[1].ts >= window[0].ts);
    }
}

// ===========================================================================
// 2. Stream completeness (tests 21–35)
// ===========================================================================

#[tokio::test]
async fn completeness_all_events_received() {
    let (tx, rx) = mpsc::channel(32);
    let count = 25;
    for i in 0..count {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), count);
}

#[tokio::test]
async fn completeness_no_duplicates() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..10 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("unique-{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let unique: std::collections::HashSet<_> = texts.iter().collect();
    assert_eq!(texts.len(), unique.len());
}

#[tokio::test]
async fn completeness_terminal_event_present() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::RunCompleted {
        message: "ok".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let has_terminal = events.iter().any(|e| kind_name(e) == "run_completed");
    assert!(has_terminal);
}

#[tokio::test]
async fn completeness_channel_close_signals_end() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn completeness_large_stream() {
    let (tx, rx) = mpsc::channel(1024);
    let n = 1000;
    for i in 0..n {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("t{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), n);
}

#[tokio::test]
async fn completeness_all_event_kinds_representable() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "T".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "T".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "a".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
        AgentEventKind::RunCompleted {
            message: "d".into(),
        },
    ];
    let (tx, rx) = mpsc::channel(16);
    for k in &kinds {
        tx.send(make_event(k.clone())).await.unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn completeness_trace_matches_stream() {
    let mut trace = Vec::new();
    let (tx, rx) = mpsc::channel(16);
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        AgentEventKind::AssistantDelta { text: "hi".into() },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ];
    for k in kinds {
        let ev = make_event(k);
        trace.push(ev.clone());
        tx.send(ev).await.unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), trace.len());
    for (a, b) in events.iter().zip(trace.iter()) {
        assert_eq!(kind_name(a), kind_name(b));
    }
}

#[tokio::test]
async fn completeness_dropped_sender_closes_stream() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let events = collect_events(rx).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn completeness_receiver_gets_all_before_drop() {
    let (tx, rx) = mpsc::channel(4);
    for i in 0..4 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn completeness_event_count_matches_send_count() {
    let counts = [1, 5, 10, 50];
    for &n in &counts {
        let (tx, rx) = mpsc::channel(n + 1);
        for i in 0..n {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
        drop(tx);
        let events = collect_events(rx).await;
        assert_eq!(events.len(), n);
    }
}

#[tokio::test]
async fn completeness_interleaved_kinds_counted() {
    let (tx, rx) = mpsc::channel(32);
    for _ in 0..5 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: "d".into(),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::Warning {
            message: "w".into(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 10);
    let deltas = events
        .iter()
        .filter(|e| kind_name(e) == "assistant_delta")
        .count();
    let warnings = events.iter().filter(|e| kind_name(e) == "warning").count();
    assert_eq!(deltas, 5);
    assert_eq!(warnings, 5);
}

#[tokio::test]
async fn completeness_stream_with_only_terminal() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 1);
    assert_eq!(kind_name(&events[0]), "run_completed");
}

#[tokio::test]
async fn completeness_stream_with_error_terminal() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Error {
        message: "fatal".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
    assert_eq!(kind_name(&events[1]), "error");
}

#[tokio::test]
async fn completeness_no_events_lost_under_capacity() {
    let (tx, rx) = mpsc::channel(2);
    let sender = tokio::spawn(async move {
        for i in 0..10 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let events = collect_events(rx).await;
    sender.await.unwrap();
    assert_eq!(events.len(), 10);
}

// ===========================================================================
// 3. Partial content / delta accumulation (tests 36–50)
// ===========================================================================

#[tokio::test]
async fn partial_deltas_accumulate_to_full() {
    let fragments = vec!["Hello", ", ", "world", "!"];
    let (tx, rx) = mpsc::channel(16);
    for f in &fragments {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: (*f).to_string(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "Hello, world!");
}

#[tokio::test]
async fn partial_empty_deltas_ignored_in_accumulation() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: String::new(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "ab");
}

#[tokio::test]
async fn partial_single_char_deltas() {
    let word = "streaming";
    let (tx, rx) = mpsc::channel(16);
    for ch in word.chars() {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: ch.to_string(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, word);
}

#[tokio::test]
async fn partial_unicode_deltas() {
    let fragments = vec!["こんに", "ちは", "🌍"];
    let (tx, rx) = mpsc::channel(16);
    for f in &fragments {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: (*f).to_string(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "こんにちは🌍");
}

#[tokio::test]
async fn partial_newlines_in_deltas() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "line1\n".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "line2\n".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "line1\nline2\n");
}

#[tokio::test]
async fn partial_whitespace_only_deltas() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "  ".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "\t".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "  \t");
}

#[tokio::test]
async fn partial_large_delta_payload() {
    let big = "x".repeat(10_000);
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: big.clone(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::AssistantDelta { text } = &events[0].kind {
        assert_eq!(text.len(), 10_000);
    }
}

#[tokio::test]
async fn partial_delta_count() {
    let n = 50;
    let (tx, rx) = mpsc::channel(64);
    for i in 0..n {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let delta_count = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
        .count();
    assert_eq!(delta_count, n);
}

#[tokio::test]
async fn partial_deltas_mixed_with_tool_events() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "before ".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "T".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "T".into(),
        tool_use_id: None,
        output: json!("r"),
        is_error: false,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "after".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "before after");
}

#[tokio::test]
async fn partial_repeated_identical_deltas() {
    let (tx, rx) = mpsc::channel(16);
    for _ in 0..5 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: "same".into(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 5);
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "samesamesamesamesame");
}

#[tokio::test]
async fn partial_message_after_deltas() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "He".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "llo".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantMessage {
        text: "Hello".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let delta_text: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let full = events
        .iter()
        .find_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(delta_text, full);
}

#[tokio::test]
async fn partial_code_block_deltas() {
    let parts = vec![
        "```rust\n",
        "fn main() {\n",
        "    println!(\"hi\");\n",
        "}\n",
        "```",
    ];
    let (tx, rx) = mpsc::channel(16);
    for p in &parts {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: (*p).to_string(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(assembled.contains("fn main()"));
    assert!(assembled.starts_with("```rust"));
}

#[tokio::test]
async fn partial_special_chars_in_deltas() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "<div>".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "&amp;".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "</div>".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(assembled, "<div>&amp;</div>");
}

#[tokio::test]
async fn partial_json_fragment_deltas() {
    let parts = vec!["{\"key\":", " \"va", "lue\"", "}"];
    let (tx, rx) = mpsc::channel(16);
    for p in &parts {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: (*p).to_string(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    let assembled: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let parsed: serde_json::Value = serde_json::from_str(&assembled).unwrap();
    assert_eq!(parsed["key"], "value");
}

// ===========================================================================
// 4. Tool call lifecycle (tests 51–65)
// ===========================================================================

#[tokio::test]
async fn tool_lifecycle_call_then_result() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tc1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "main.rs"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: Some("tc1".into()),
        output: json!("fn main() {}"),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn tool_lifecycle_ids_correlate() {
    let (tx, rx) = mpsc::channel(16);
    let call_id = "tool-42";
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Write".into(),
        tool_use_id: Some(call_id.into()),
        parent_tool_use_id: None,
        input: json!({}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Write".into(),
        tool_use_id: Some(call_id.into()),
        output: json!("ok"),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let call_tid = match &events[0].kind {
        AgentEventKind::ToolCall { tool_use_id, .. } => tool_use_id.clone(),
        _ => panic!("expected ToolCall"),
    };
    let result_tid = match &events[1].kind {
        AgentEventKind::ToolResult { tool_use_id, .. } => tool_use_id.clone(),
        _ => panic!("expected ToolResult"),
    };
    assert_eq!(call_tid, result_tid);
}

#[tokio::test]
async fn tool_lifecycle_error_result() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Bash".into(),
        tool_use_id: Some("tc-err".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "exit 1"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Bash".into(),
        tool_use_id: Some("tc-err".into()),
        output: json!("command failed"),
        is_error: true,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolResult { is_error, .. } = &events[1].kind {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[tokio::test]
async fn tool_lifecycle_multiple_tools_sequential() {
    let (tx, rx) = mpsc::channel(32);
    let tools = vec!["Read", "Write", "Bash", "Glob", "Grep"];
    for (i, tool) in tools.iter().enumerate() {
        let id = format!("tc-{i}");
        tx.send(make_event(AgentEventKind::ToolCall {
            tool_name: (*tool).to_string(),
            tool_use_id: Some(id.clone()),
            parent_tool_use_id: None,
            input: json!({"idx": i}),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::ToolResult {
            tool_name: (*tool).to_string(),
            tool_use_id: Some(id),
            output: json!("ok"),
            is_error: false,
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn tool_lifecycle_no_tool_use_id() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "T".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "T".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn tool_lifecycle_tool_name_preserved() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "my_custom_tool".into(),
        tool_use_id: Some("tc".into()),
        parent_tool_use_id: None,
        input: json!({"a": 1}),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolCall { tool_name, .. } = &events[0].kind {
        assert_eq!(tool_name, "my_custom_tool");
    }
}

#[tokio::test]
async fn tool_lifecycle_input_json_preserved() {
    let input = json!({"path": "/tmp/test.rs", "recursive": true, "depth": 3});
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Glob".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: input.clone(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolCall {
        input: recv_input, ..
    } = &events[0].kind
    {
        assert_eq!(*recv_input, input);
    }
}

#[tokio::test]
async fn tool_lifecycle_output_json_preserved() {
    let output = json!({"files": ["a.rs", "b.rs"], "count": 2});
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Glob".into(),
        tool_use_id: None,
        output: output.clone(),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolResult {
        output: recv_out, ..
    } = &events[0].kind
    {
        assert_eq!(*recv_out, output);
    }
}

#[tokio::test]
async fn tool_lifecycle_nested_tool_parent_id() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Inner".into(),
        tool_use_id: Some("child-1".into()),
        parent_tool_use_id: Some("parent-1".into()),
        input: json!({}),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &events[0].kind
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
    }
}

#[tokio::test]
async fn tool_lifecycle_file_changed_after_write() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Write".into(),
        tool_use_id: Some("w".into()),
        parent_tool_use_id: None,
        input: json!({"path": "new.rs"}),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Write".into(),
        tool_use_id: Some("w".into()),
        output: json!("ok"),
        is_error: false,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::FileChanged {
        path: "new.rs".into(),
        summary: "created file".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 3);
    assert_eq!(kind_name(&events[2]), "file_changed");
    if let AgentEventKind::FileChanged { path, .. } = &events[2].kind {
        assert_eq!(path, "new.rs");
    }
}

#[tokio::test]
async fn tool_lifecycle_command_with_exit_code() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(0),
        output_preview: Some("Compiling...".into()),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::CommandExecuted {
        exit_code,
        output_preview,
        ..
    } = &events[0].kind
    {
        assert_eq!(*exit_code, Some(0));
        assert_eq!(output_preview.as_deref(), Some("Compiling..."));
    }
}

#[tokio::test]
async fn tool_lifecycle_command_no_exit_code() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::CommandExecuted {
        command: "timeout".into(),
        exit_code: None,
        output_preview: None,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::CommandExecuted {
        exit_code,
        output_preview,
        ..
    } = &events[0].kind
    {
        assert!(exit_code.is_none());
        assert!(output_preview.is_none());
    }
}

#[tokio::test]
async fn tool_lifecycle_many_file_changes() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..10 {
        tx.send(make_event(AgentEventKind::FileChanged {
            path: format!("file{i}.rs"),
            summary: format!("modified file {i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 10);
    for (i, ev) in events.iter().enumerate() {
        if let AgentEventKind::FileChanged { path, .. } = &ev.kind {
            assert_eq!(*path, format!("file{i}.rs"));
        }
    }
}

#[tokio::test]
async fn tool_lifecycle_tool_result_large_output() {
    let big_output = json!({"data": "x".repeat(5000)});
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "Read".into(),
        tool_use_id: None,
        output: big_output.clone(),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolResult { output, .. } = &events[0].kind {
        assert_eq!(*output, big_output);
    }
}

// ===========================================================================
// 5. Error propagation (tests 66–80)
// ===========================================================================

#[tokio::test]
async fn error_event_message_preserved() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Error {
        message: "something went wrong".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::Error { message, .. } = &events[0].kind {
        assert_eq!(message, "something went wrong");
    }
}

#[tokio::test]
async fn error_event_with_error_code() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Error {
        message: "bad envelope".into(),
        error_code: Some(abp_error::ErrorCode::ProtocolInvalidEnvelope),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::Error { error_code, .. } = &events[0].kind {
        assert_eq!(
            *error_code,
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }
}

#[tokio::test]
async fn error_multiple_errors_in_stream() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..3 {
        tx.send(make_event(AgentEventKind::Error {
            message: format!("error-{i}"),
            error_code: None,
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 3);
    for ev in &events {
        assert_eq!(kind_name(ev), "error");
    }
}

#[tokio::test]
async fn error_after_successful_events() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "hi".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 3);
    assert_eq!(kind_name(&events[2]), "error");
}

#[tokio::test]
async fn error_warning_is_not_error() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Warning {
        message: "warn".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(kind_name(&events[0]), "warning");
    assert_ne!(kind_name(&events[0]), "error");
}

#[tokio::test]
async fn error_fatal_envelope_round_trip() {
    let envelope = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { error, ref_id, .. } = decoded {
        assert_eq!(error, "boom");
        assert_eq!(ref_id, Some("run-1".to_string()));
    } else {
        panic!("expected Fatal");
    }
}

#[tokio::test]
async fn error_fatal_with_error_code_envelope() {
    let envelope = Envelope::fatal_with_code(
        Some("run-2".into()),
        "protocol error",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(decoded.error_code().is_some());
}

#[tokio::test]
async fn error_tool_result_is_error_flag() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "T".into(),
        tool_use_id: None,
        output: json!("fail"),
        is_error: true,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::ToolResult {
        tool_name: "T".into(),
        tool_use_id: None,
        output: json!("ok"),
        is_error: false,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::ToolResult { is_error, .. } = &events[0].kind {
        assert!(is_error);
    }
    if let AgentEventKind::ToolResult { is_error, .. } = &events[1].kind {
        assert!(!is_error);
    }
}

#[tokio::test]
async fn error_event_serde_round_trip() {
    let ev = make_event(AgentEventKind::Error {
        message: "serde test".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { message, .. } = &back.kind {
        assert_eq!(message, "serde test");
    }
}

#[tokio::test]
async fn error_warning_serde_round_trip() {
    let ev = make_event(AgentEventKind::Warning {
        message: "warn test".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Warning { message } = &back.kind {
        assert_eq!(message, "warn test");
    }
}

#[tokio::test]
async fn error_empty_message() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Error {
        message: String::new(),
        error_code: None,
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    if let AgentEventKind::Error { message, .. } = &events[0].kind {
        assert!(message.is_empty());
    }
}

#[tokio::test]
async fn error_interleaved_with_warnings() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::Warning {
        message: "w1".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Error {
        message: "e1".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::Warning {
        message: "w2".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    let names: Vec<_> = events.iter().map(kind_name).collect();
    assert_eq!(names, vec!["warning", "error", "warning"]);
}

#[tokio::test]
async fn error_fatal_envelope_no_ref_id() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "unknown run".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { ref_id, .. } = decoded {
        assert!(ref_id.is_none());
    }
}

#[tokio::test]
async fn error_protocol_error_decode_invalid() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

// ===========================================================================
// 6. Backpressure (tests 81–95)
// ===========================================================================

#[tokio::test]
async fn backpressure_small_channel_no_loss() {
    let (tx, rx) = mpsc::channel(1);
    let handle = tokio::spawn(async move {
        for i in 0..20 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let events = collect_events(rx).await;
    handle.await.unwrap();
    assert_eq!(events.len(), 20);
}

#[tokio::test]
async fn backpressure_sender_waits_for_capacity() {
    let (tx, rx) = mpsc::channel(2);
    let sent = Arc::new(AtomicUsize::new(0));
    let sent_clone = sent.clone();
    let sender = tokio::spawn(async move {
        for i in 0..10 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
            sent_clone.fetch_add(1, Ordering::SeqCst);
        }
    });
    let events = collect_events(rx).await;
    sender.await.unwrap();
    assert_eq!(events.len(), 10);
    assert_eq!(sent.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn backpressure_slow_consumer() {
    let (tx, rx) = mpsc::channel(4);
    let sender = tokio::spawn(async move {
        for i in 0..30 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let mut events = Vec::new();
    let mut rx = rx;
    while let Some(ev) = rx.recv().await {
        events.push(ev);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    sender.await.unwrap();
    assert_eq!(events.len(), 30);
}

#[tokio::test]
async fn backpressure_channel_capacity_1() {
    let (tx, rx) = mpsc::channel(1);
    let sender = tokio::spawn(async move {
        for i in 0..5 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let events = collect_events(rx).await;
    sender.await.unwrap();
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn backpressure_large_channel_fast_path() {
    let (tx, rx) = mpsc::channel(1000);
    for i in 0..500 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 500);
}

#[tokio::test]
async fn backpressure_producer_consumer_balanced() {
    let (tx, rx) = mpsc::channel(8);
    let producer = tokio::spawn(async move {
        for i in 0..100 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let consumer = tokio::spawn(async move { collect_events(rx).await });
    producer.await.unwrap();
    let events = consumer.await.unwrap();
    assert_eq!(events.len(), 100);
}

#[tokio::test]
async fn backpressure_try_send_full_channel() {
    let (tx, _rx) = mpsc::channel(1);
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    let result = tx.try_send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }));
    assert!(result.is_err());
}

#[tokio::test]
async fn backpressure_dropped_receiver_send_fails() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(rx);
    let result = tx
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "x".into(),
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn backpressure_multiple_senders_no_loss() {
    let (tx, rx) = mpsc::channel(16);
    let mut handles = Vec::new();
    for s in 0..4 {
        let tx_c = tx.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..10 {
                tx_c.send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("s{s}-{i}"),
                }))
                .await
                .unwrap();
            }
        }));
    }
    drop(tx);
    let collector = tokio::spawn(async move { collect_events(rx).await });
    for h in handles {
        h.await.unwrap();
    }
    let events = collector.await.unwrap();
    assert_eq!(events.len(), 40);
}

#[tokio::test]
async fn backpressure_burst_then_drain() {
    let (tx, rx) = mpsc::channel(64);
    for i in 0..64 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 64);
}

#[tokio::test]
async fn backpressure_interleaved_send_recv() {
    let (tx, mut rx) = mpsc::channel(4);
    let sender = tokio::spawn(async move {
        for i in 0..20 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let mut count = 0;
    while let Some(_ev) = rx.recv().await {
        count += 1;
    }
    sender.await.unwrap();
    assert_eq!(count, 20);
}

#[tokio::test]
async fn backpressure_zero_events() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let events = collect_events(rx).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn backpressure_exact_capacity_fill() {
    let cap = 16;
    let (tx, rx) = mpsc::channel(cap);
    for i in 0..cap {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), cap);
}

#[tokio::test]
async fn backpressure_rapid_open_close() {
    for _ in 0..50 {
        let (tx, rx) = mpsc::channel(4);
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: "x".into(),
        }))
        .await
        .unwrap();
        drop(tx);
        let events = collect_events(rx).await;
        assert_eq!(events.len(), 1);
    }
}

#[tokio::test]
async fn backpressure_concurrent_senders_ordering_per_sender() {
    let (tx, rx) = mpsc::channel(32);
    let tx2 = tx.clone();
    let h1 = tokio::spawn(async move {
        for i in 0..10 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("A{i}"),
            }))
            .await
            .unwrap();
        }
    });
    let h2 = tokio::spawn(async move {
        for i in 0..10 {
            tx2.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("B{i}"),
            }))
            .await
            .unwrap();
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 20);
    // Verify per-sender order is preserved
    let a_events: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } if text.starts_with('A') => Some(text.clone()),
            _ => None,
        })
        .collect();
    let b_events: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } if text.starts_with('B') => Some(text.clone()),
            _ => None,
        })
        .collect();
    for (i, t) in a_events.iter().enumerate() {
        assert_eq!(*t, format!("A{i}"));
    }
    for (i, t) in b_events.iter().enumerate() {
        assert_eq!(*t, format!("B{i}"));
    }
}

// ===========================================================================
// 7. Multiple concurrent streams / isolation (tests 96–110)
// ===========================================================================

#[tokio::test]
async fn concurrent_two_streams_isolated() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(make_event(AgentEventKind::AssistantDelta {
        text: "stream1".into(),
    }))
    .await
    .unwrap();
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "stream2".into(),
    }))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 1);
    assert_eq!(e2.len(), 1);
    if let AgentEventKind::AssistantDelta { text } = &e1[0].kind {
        assert_eq!(text, "stream1");
    }
    if let AgentEventKind::AssistantDelta { text } = &e2[0].kind {
        assert_eq!(text, "stream2");
    }
}

#[tokio::test]
async fn concurrent_many_streams_no_cross_talk() {
    let n_streams = 10;
    let mut handles = Vec::new();
    let mut receivers = Vec::new();
    for s in 0..n_streams {
        let (tx, rx) = mpsc::channel(16);
        receivers.push(rx);
        handles.push(tokio::spawn(async move {
            for i in 0..5 {
                tx.send(make_event(AgentEventKind::AssistantDelta {
                    text: format!("s{s}-e{i}"),
                }))
                .await
                .unwrap();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    for (s, rx) in receivers.into_iter().enumerate() {
        let events = collect_events(rx).await;
        assert_eq!(events.len(), 5);
        for (i, ev) in events.iter().enumerate() {
            if let AgentEventKind::AssistantDelta { text } = &ev.kind {
                assert_eq!(*text, format!("s{s}-e{i}"));
            }
        }
    }
}

#[tokio::test]
async fn concurrent_streams_different_event_types() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    }))
    .await
    .unwrap();
    tx2.send(make_event(AgentEventKind::AssistantMessage {
        text: "msg".into(),
    }))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(kind_name(&e1[0]), "tool_call");
    assert_eq!(kind_name(&e2[0]), "assistant_message");
}

#[tokio::test]
async fn concurrent_stream_one_fails_other_continues() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    }))
    .await
    .unwrap();
    drop(tx1);
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "ok".into(),
    }))
    .await
    .unwrap();
    tx2.send(make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }))
    .await
    .unwrap();
    drop(tx2);
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 1);
    assert_eq!(e2.len(), 2);
}

#[tokio::test]
async fn concurrent_streams_independent_timing() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let h1 = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        tx1.send(make_event(AgentEventKind::AssistantDelta {
            text: "slow".into(),
        }))
        .await
        .unwrap();
    });
    let h2 = tokio::spawn(async move {
        tx2.send(make_event(AgentEventKind::AssistantDelta {
            text: "fast".into(),
        }))
        .await
        .unwrap();
    });
    h1.await.unwrap();
    h2.await.unwrap();
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 1);
    assert_eq!(e2.len(), 1);
}

#[tokio::test]
async fn concurrent_stream_counters_independent() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let c1 = counter1.clone();
    let c2 = counter2.clone();
    let h1 = tokio::spawn(async move {
        for _ in 0..5 {
            tx1.send(make_event(AgentEventKind::AssistantDelta {
                text: "a".into(),
            }))
            .await
            .unwrap();
            c1.fetch_add(1, Ordering::SeqCst);
        }
    });
    let h2 = tokio::spawn(async move {
        for _ in 0..3 {
            tx2.send(make_event(AgentEventKind::AssistantDelta {
                text: "b".into(),
            }))
            .await
            .unwrap();
            c2.fetch_add(1, Ordering::SeqCst);
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 5);
    assert_eq!(e2.len(), 3);
    assert_eq!(counter1.load(Ordering::SeqCst), 5);
    assert_eq!(counter2.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn concurrent_stream_different_sizes() {
    let sizes = [1, 10, 50, 100];
    let mut results = Vec::new();
    for &size in &sizes {
        let (tx, rx) = mpsc::channel(size + 1);
        tokio::spawn(async move {
            for i in 0..size {
                let _ = tx
                    .send(make_event(AgentEventKind::AssistantDelta {
                        text: format!("{i}"),
                    }))
                    .await;
            }
        });
        results.push((size, rx));
    }
    for (expected, rx) in results {
        let events = collect_events(rx).await;
        assert_eq!(events.len(), expected);
    }
}

#[tokio::test]
async fn concurrent_streams_drop_one_early() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    tx1.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2); // Drop sender so receiver finishes
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 1);
    assert!(e2.is_empty());
}

#[tokio::test]
async fn concurrent_streams_all_empty() {
    let mut receivers = Vec::new();
    for _ in 0..5 {
        let (tx, rx) = mpsc::channel::<AgentEvent>(16);
        drop(tx);
        receivers.push(rx);
    }
    for rx in receivers {
        let events = collect_events(rx).await;
        assert!(events.is_empty());
    }
}

#[tokio::test]
async fn concurrent_streams_same_content_different_channels() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "same".into(),
    });
    tx1.send(ev.clone()).await.unwrap();
    tx2.send(ev).await.unwrap();
    drop(tx1);
    drop(tx2);
    let e1 = collect_events(rx1).await;
    let e2 = collect_events(rx2).await;
    assert_eq!(e1.len(), 1);
    assert_eq!(e2.len(), 1);
}

#[tokio::test]
async fn concurrent_streams_sequential_then_parallel() {
    let (tx1, rx1) = mpsc::channel(16);
    tx1.send(make_event(AgentEventKind::RunStarted {
        message: "1".into(),
    }))
    .await
    .unwrap();
    tx1.send(make_event(AgentEventKind::RunCompleted {
        message: "1".into(),
    }))
    .await
    .unwrap();
    drop(tx1);
    let e1 = collect_events(rx1).await;
    assert_eq!(e1.len(), 2);

    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);
    let h2 = tokio::spawn(async move {
        tx2.send(make_event(AgentEventKind::RunStarted {
            message: "2".into(),
        }))
        .await
        .unwrap();
    });
    let h3 = tokio::spawn(async move {
        tx3.send(make_event(AgentEventKind::RunStarted {
            message: "3".into(),
        }))
        .await
        .unwrap();
    });
    h2.await.unwrap();
    h3.await.unwrap();
    let e2 = collect_events(rx2).await;
    let e3 = collect_events(rx3).await;
    assert_eq!(e2.len(), 1);
    assert_eq!(e3.len(), 1);
}

#[tokio::test]
async fn concurrent_stream_ref_id_isolation() {
    let run_id_1 = Uuid::new_v4().to_string();
    let run_id_2 = Uuid::new_v4().to_string();
    let envelope1 = Envelope::Event {
        ref_id: run_id_1.clone(),
        event: make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
    };
    let envelope2 = Envelope::Event {
        ref_id: run_id_2.clone(),
        event: make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
    };
    let json1 = JsonlCodec::encode(&envelope1).unwrap();
    let json2 = JsonlCodec::encode(&envelope2).unwrap();
    let d1 = JsonlCodec::decode(json1.trim()).unwrap();
    let d2 = JsonlCodec::decode(json2.trim()).unwrap();
    if let (Envelope::Event { ref_id: r1, .. }, Envelope::Event { ref_id: r2, .. }) = (&d1, &d2) {
        assert_ne!(r1, r2);
        assert_eq!(*r1, run_id_1);
        assert_eq!(*r2, run_id_2);
    }
}

#[tokio::test]
async fn concurrent_fifty_streams_stress() {
    let mut handles = Vec::new();
    let mut receivers = Vec::new();
    for _ in 0..50 {
        let (tx, rx) = mpsc::channel(8);
        receivers.push(rx);
        handles.push(tokio::spawn(async move {
            for j in 0..3 {
                let _ = tx
                    .send(make_event(AgentEventKind::AssistantDelta {
                        text: format!("{j}"),
                    }))
                    .await;
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    for rx in receivers {
        let events = collect_events(rx).await;
        assert_eq!(events.len(), 3);
    }
}

// ===========================================================================
// 8. Stream cancellation (tests 111–125)
// ===========================================================================

#[tokio::test]
async fn cancel_drop_receiver_stops_producer() {
    let (tx, rx) = mpsc::channel(2);
    drop(rx);
    let result = tx
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "x".into(),
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cancel_partial_stream_events_received() {
    let (tx, mut rx) = mpsc::channel(16);
    for i in 0..5 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: format!("{i}"),
        }))
        .await
        .unwrap();
    }
    let first = rx.recv().await;
    assert!(first.is_some());
    drop(rx);
    // Sender should detect closed channel
    let result = tx
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "x".into(),
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cancel_abort_spawned_task() {
    let (tx, rx) = mpsc::channel(16);
    let handle = tokio::spawn(async move {
        loop {
            if tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: "loop".into(),
                }))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    handle.abort();
    let _ = handle.await;
    let events = collect_events(rx).await;
    assert!(!events.is_empty());
}

#[tokio::test]
async fn cancel_timeout_on_slow_producer() {
    let (tx, mut rx) = mpsc::channel(16);
    let _handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let _ = tx
            .send(make_event(AgentEventKind::AssistantDelta {
                text: "late".into(),
            }))
            .await;
    });
    let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(result.is_err()); // Timed out
}

#[tokio::test]
async fn cancel_sender_dropped_mid_stream() {
    let (tx, rx) = mpsc::channel(16);
    let handle = tokio::spawn(async move {
        for i in 0..3 {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            }))
            .await
            .unwrap();
        }
        // drop sender here (implicitly)
    });
    handle.await.unwrap();
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn cancel_receiver_closed_returns_error() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(1);
    drop(rx);
    assert!(tx.is_closed());
}

#[tokio::test]
async fn cancel_try_recv_empty() {
    let (_tx, mut rx) = mpsc::channel::<AgentEvent>(16);
    let result = rx.try_recv();
    assert!(result.is_err());
}

#[tokio::test]
async fn cancel_multiple_receivers_drop_one() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    drop(rx1);
    assert!(tx1
        .send(make_event(AgentEventKind::AssistantDelta {
            text: "x".into()
        }))
        .await
        .is_err());
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "y".into(),
    }))
    .await
    .unwrap();
    drop(tx2);
    let events = collect_events(rx2).await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn cancel_select_first_stream() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    tx1.send(make_event(AgentEventKind::AssistantDelta {
        text: "first".into(),
    }))
    .await
    .unwrap();
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "second".into(),
    }))
    .await
    .unwrap();
    let winner = tokio::select! {
        Some(ev) = rx1.recv() => kind_name(&ev),
        Some(ev) = rx2.recv() => kind_name(&ev),
    };
    assert_eq!(winner, "assistant_delta");
}

#[tokio::test]
async fn cancel_graceful_shutdown_with_terminal() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "working".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 3);
    assert_eq!(kind_name(events.last().unwrap()), "run_completed");
}

#[tokio::test]
async fn cancel_abort_mid_tool_call() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(make_event(AgentEventKind::ToolCall {
        tool_name: "Bash".into(),
        tool_use_id: Some("tc".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "long running"}),
    }))
    .await
    .unwrap();
    drop(tx); // Cancel before result
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 1);
    assert_eq!(kind_name(&events[0]), "tool_call");
}

#[tokio::test]
async fn cancel_channel_closed_check() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    assert!(!tx.is_closed());
    drop(rx);
    assert!(tx.is_closed());
}

#[tokio::test]
async fn cancel_sender_clone_last_drop_closes() {
    let (tx, rx) = mpsc::channel(16);
    let tx2 = tx.clone();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx2);
    let events = collect_events(rx).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn cancel_task_join_after_abort() {
    let (tx, rx) = mpsc::channel(16);
    let handle = tokio::spawn(async move {
        loop {
            if tx
                .send(make_event(AgentEventKind::AssistantDelta {
                    text: "x".into(),
                }))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    handle.abort();
    let result = handle.await;
    assert!(result.is_err()); // JoinError::Cancelled
    let events = collect_events(rx).await;
    assert!(!events.is_empty());
}

// ===========================================================================
// 9. Receipt generation (tests 126–140)
// ===========================================================================

#[tokio::test]
async fn receipt_builder_creates_valid_receipt() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn receipt_hash_is_deterministic() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn receipt_with_hash_sets_sha256() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_trace_includes_stream_events() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let receipt = simple_receipt(events.clone());
    assert_eq!(receipt.trace.len(), 3);
}

#[tokio::test]
async fn receipt_contract_version_matches() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn receipt_outcome_complete() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn receipt_outcome_partial() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[tokio::test]
async fn receipt_outcome_failed() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[tokio::test]
async fn receipt_serde_round_trip() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(back.outcome, receipt.outcome);
}

#[tokio::test]
async fn receipt_envelope_round_trip() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let envelope = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: receipt.clone(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt: r, ref_id } = decoded {
        assert_eq!(ref_id, "run-1");
        assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
    } else {
        panic!("expected Final");
    }
}

#[tokio::test]
async fn receipt_empty_trace() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert!(receipt.trace.is_empty());
}

#[tokio::test]
async fn receipt_with_artifacts() {
    let receipt = ReceiptBuilder::new("mock")
        .add_artifact(abp_core::ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[tokio::test]
async fn receipt_duration_zero_for_same_timestamps() {
    let now = Utc::now();
    let receipt = ReceiptBuilder::new("mock")
        .started_at(now)
        .finished_at(now)
        .build();
    assert_eq!(receipt.meta.duration_ms, 0);
}

#[tokio::test]
async fn receipt_backend_identity_preserved() {
    let receipt = ReceiptBuilder::new("my-backend")
        .backend_version("2.0")
        .adapter_version("1.0")
        .build();
    assert_eq!(receipt.backend.id, "my-backend");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("1.0"));
}

#[tokio::test]
async fn receipt_capabilities_in_receipt() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let receipt = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    assert!(receipt.capabilities.contains_key(&Capability::ToolRead));
}

// ===========================================================================
// 10. Event metadata (tests 141–155)
// ===========================================================================

#[tokio::test]
async fn metadata_timestamp_present() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let now = Utc::now();
    let diff = now - ev.ts;
    assert!(diff.num_seconds() < 2);
}

#[tokio::test]
async fn metadata_ext_field_default_none() {
    let ev = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
    assert!(ev.ext.is_none());
}

#[tokio::test]
async fn metadata_ext_field_with_data() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));
    let ev = make_event_with_ext(AgentEventKind::AssistantDelta { text: "x".into() }, ext);
    assert!(ev.ext.is_some());
    assert!(ev.ext.as_ref().unwrap().contains_key("raw_message"));
}

#[tokio::test]
async fn metadata_event_serde_preserves_timestamp() {
    let ev = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev.ts, back.ts);
}

#[tokio::test]
async fn metadata_event_serde_preserves_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("key".to_string(), json!("value"));
    let ev = make_event_with_ext(AgentEventKind::AssistantMessage { text: "m".into() }, ext);
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ext.unwrap()["key"], "value");
}

#[tokio::test]
async fn metadata_run_id_in_receipt() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_ne!(receipt.meta.run_id, Uuid::nil());
}

#[tokio::test]
async fn metadata_work_order_id_in_receipt() {
    let wo_id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("mock").work_order_id(wo_id).build();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn metadata_envelope_ref_id_preserved() {
    let ref_id = Uuid::new_v4().to_string();
    let envelope = Envelope::Event {
        ref_id: ref_id.clone(),
        event: make_event(AgentEventKind::AssistantDelta { text: "x".into() }),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { ref_id: r, .. } = decoded {
        assert_eq!(r, ref_id);
    }
}

#[tokio::test]
async fn metadata_hello_envelope_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = &hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    }
}

#[tokio::test]
async fn metadata_hello_envelope_serde() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[tokio::test]
async fn metadata_run_envelope_contains_work_order() {
    let wo = simple_work_order();
    let envelope = Envelope::Run {
        id: "run-1".into(),
        work_order: wo.clone(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-1");
        assert_eq!(work_order.task, wo.task);
    }
}

#[tokio::test]
async fn metadata_event_envelope_newline_terminated() {
    let envelope = Envelope::Event {
        ref_id: "r".into(),
        event: make_event(AgentEventKind::AssistantDelta { text: "x".into() }),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(json.ends_with('\n'));
}

#[tokio::test]
async fn metadata_timestamps_ordered_in_stream() {
    let (tx, rx) = mpsc::channel(16);
    for _ in 0..10 {
        tx.send(make_event(AgentEventKind::AssistantDelta {
            text: "t".into(),
        }))
        .await
        .unwrap();
    }
    drop(tx);
    let events = collect_events(rx).await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn metadata_execution_mode_default() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn metadata_execution_mode_passthrough() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// 11. Cross-dialect event fidelity (tests 156–170)
// ===========================================================================

#[tokio::test]
async fn dialect_event_round_trip_all_kinds() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        AgentEventKind::RunCompleted {
            message: "ok".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "T".into(),
            tool_use_id: Some("id".into()),
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "T".into(),
            tool_use_id: Some("id".into()),
            output: json!("out"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f.rs".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let ev = make_event(kind);
        let json = serde_json::to_string(&ev).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(kind_name(&ev), kind_name(&back));
    }
}

#[tokio::test]
async fn dialect_envelope_event_preserves_all_fields() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "Read".into(),
        tool_use_id: Some("tc-99".into()),
        parent_tool_use_id: Some("parent-1".into()),
        input: json!({"path": "/src/main.rs", "recursive": true}),
    });
    let envelope = Envelope::Event {
        ref_id: "run-abc".into(),
        event: ev,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "run-abc");
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } = &event.kind
        {
            assert_eq!(tool_name, "Read");
            assert_eq!(tool_use_id.as_deref(), Some("tc-99"));
            assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
            assert_eq!(input["path"], "/src/main.rs");
        } else {
            panic!("expected ToolCall");
        }
    }
}

#[tokio::test]
async fn dialect_stream_encode_decode_sequence() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let run_id = "run-xyz";
    let mut encoded = String::new();
    for ev in &events {
        let env = Envelope::Event {
            ref_id: run_id.into(),
            event: ev.clone(),
        };
        encoded.push_str(&JsonlCodec::encode(&env).unwrap());
    }
    let reader = std::io::BufReader::new(encoded.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
    for d in &decoded {
        if let Envelope::Event { ref_id, .. } = d {
            assert_eq!(ref_id, run_id);
        }
    }
}

#[tokio::test]
async fn dialect_passthrough_ext_preserved() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        }),
    );
    let ev = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext.clone(),
    );
    let envelope = Envelope::Event {
        ref_id: "r".into(),
        event: ev,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        let decoded_ext = event.ext.unwrap();
        assert_eq!(decoded_ext["raw_message"], ext["raw_message"]);
    }
}

#[tokio::test]
async fn dialect_multiple_streams_different_backends() {
    let backends = ["claude", "openai", "gemini", "copilot", "codex"];
    for backend in &backends {
        let (tx, rx) = mpsc::channel(16);
        tx.send(make_event(AgentEventKind::RunStarted {
            message: format!("{backend} start"),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::AssistantMessage {
            text: format!("response from {backend}"),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::RunCompleted {
            message: format!("{backend} done"),
        }))
        .await
        .unwrap();
        drop(tx);
        let events = collect_events(rx).await;
        assert_eq!(events.len(), 3);
        if let AgentEventKind::RunStarted { message } = &events[0].kind {
            assert!(message.contains(backend));
        }
    }
}

#[tokio::test]
async fn dialect_receipt_hash_independent_of_backend() {
    let r1 = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("openai")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    // Different backends produce different hashes
    assert_ne!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert_eq!(h2.len(), 64);
}

#[tokio::test]
async fn dialect_work_order_serde_fidelity() {
    let wo = WorkOrderBuilder::new("test task")
        .model("gpt-4")
        .max_turns(10)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "test task");
    assert_eq!(back.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(back.config.max_turns, Some(10));
}

#[tokio::test]
async fn dialect_capability_manifest_in_hello() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        caps,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert!(capabilities.contains_key(&Capability::ToolUse));
        assert!(capabilities.contains_key(&Capability::ExtendedThinking));
    }
}

#[tokio::test]
async fn dialect_error_code_round_trip_in_event() {
    let ev = make_event(AgentEventKind::Error {
        message: "protocol error".into(),
        error_code: Some(abp_error::ErrorCode::ProtocolUnexpectedMessage),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &back.kind {
        assert_eq!(
            *error_code,
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );
    }
}

#[tokio::test]
async fn dialect_full_protocol_handshake_flow() {
    let mut wire = String::new();

    // 1. Hello
    let hello = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    wire.push_str(&JsonlCodec::encode(&hello).unwrap());

    // 2. Run
    let wo = simple_work_order();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    wire.push_str(&JsonlCodec::encode(&run).unwrap());

    // 3. Events
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    for ev in &events {
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event: ev.clone(),
        };
        wire.push_str(&JsonlCodec::encode(&env).unwrap());
    }

    // 4. Final
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let final_env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    wire.push_str(&JsonlCodec::encode(&final_env).unwrap());

    // Decode entire wire
    let reader = std::io::BufReader::new(wire.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 6); // hello + run + 3 events + final
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Event { .. }));
    assert!(matches!(decoded[4], Envelope::Event { .. }));
    assert!(matches!(decoded[5], Envelope::Final { .. }));
}
