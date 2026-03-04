// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for stream aggregation utilities.

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::aggregate::{StreamAggregator, StreamSummary, ToolCallAggregate};
use chrono::Utc;

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

fn delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

fn run_started() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".to_string(),
    })
}

fn run_completed() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".to_string(),
    })
}

fn tool_call(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input,
    })
}

fn tool_result(id: &str, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: String::new(),
        tool_use_id: Some(id.to_string()),
        output,
        is_error: false,
    })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn warning_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
    })
}

fn file_changed_event(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

fn command_event(cmd: &str) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(0),
        output_preview: None,
    })
}

// ---------------------------------------------------------------------------
// StreamAggregator — basic lifecycle
// ---------------------------------------------------------------------------

#[test]
fn new_aggregator_is_empty() {
    let agg = StreamAggregator::new();
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
    assert!(agg.thinking().is_none());
    assert!(agg.errors().is_empty());
    assert!(!agg.is_complete());
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn default_aggregator_is_empty() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.event_count(), 0);
    assert_eq!(agg.text(), "");
}

#[test]
fn push_increments_event_count() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    assert_eq!(agg.event_count(), 1);
    agg.push(&delta("hi"));
    assert_eq!(agg.event_count(), 2);
}

// ---------------------------------------------------------------------------
// Text assembly
// ---------------------------------------------------------------------------

#[test]
fn assembles_text_from_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello"));
    agg.push(&delta(", "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello, world!");
}

#[test]
fn assembles_text_from_assistant_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Complete message."));
    assert_eq!(agg.text(), "Complete message.");
}

#[test]
fn mixed_deltas_and_messages() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Part 1. "));
    agg.push(&message("Part 2."));
    assert_eq!(agg.text(), "Part 1. Part 2.");
}

#[test]
fn empty_deltas_do_not_affect_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta(""));
    agg.push(&delta("a"));
    agg.push(&delta(""));
    assert_eq!(agg.text(), "a");
}

// ---------------------------------------------------------------------------
// Tool call aggregation
// ---------------------------------------------------------------------------

#[test]
fn single_tool_call_aggregated() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call(
        "read_file",
        "tc1",
        serde_json::json!({"path": "foo.rs"}),
    ));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert_eq!(agg.tool_calls()[0].id, "tc1");
    assert!(agg.tool_calls()[0].result.is_none());
}

#[test]
fn tool_result_linked_to_call() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call(
        "grep",
        "tc2",
        serde_json::json!({"pattern": "TODO"}),
    ));
    agg.push(&tool_result("tc2", serde_json::json!("found 3 matches")));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(
        agg.tool_calls()[0].result.as_deref(),
        Some("\"found 3 matches\"")
    );
}

#[test]
fn multiple_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read", "t1", serde_json::json!({})));
    agg.push(&tool_call("write", "t2", serde_json::json!({})));
    agg.push(&tool_call("exec", "t3", serde_json::json!({})));
    assert_eq!(agg.tool_calls().len(), 3);
    assert_eq!(agg.tool_calls()[0].name, "read");
    assert_eq!(agg.tool_calls()[1].name, "write");
    assert_eq!(agg.tool_calls()[2].name, "exec");
}

#[test]
fn tool_result_without_matching_call_is_ignored() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result("unknown_id", serde_json::json!("data")));
    assert!(agg.tool_calls().is_empty());
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn tool_call_without_id_gets_synthetic_key() {
    let mut agg = StreamAggregator::new();
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "my_tool".to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({"x": 1}),
    });
    agg.push(&ev);
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "my_tool");
    assert!(agg.tool_calls()[0].id.starts_with("my_tool_"));
}

#[test]
fn tool_result_without_id_is_ignored() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("tool", "t1", serde_json::json!({})));
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "tool".to_string(),
        tool_use_id: None,
        output: serde_json::json!("result"),
        is_error: false,
    });
    agg.push(&ev);
    // Result not linked because tool_use_id is None
    assert!(agg.tool_calls()[0].result.is_none());
}

// ---------------------------------------------------------------------------
// Thinking
// ---------------------------------------------------------------------------

#[test]
fn thinking_is_none_without_thinking_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    assert!(agg.thinking().is_none());
}

// ---------------------------------------------------------------------------
// Error collection
// ---------------------------------------------------------------------------

#[test]
fn errors_collected() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_event("fail 1"));
    agg.push(&error_event("fail 2"));
    assert_eq!(agg.errors().len(), 2);
}

#[test]
fn warnings_not_in_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&warning_event("just a warning"));
    assert!(agg.errors().is_empty());
}

// ---------------------------------------------------------------------------
// Completion tracking
// ---------------------------------------------------------------------------

#[test]
fn is_complete_after_run_completed() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn not_complete_without_run_completed() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("text"));
    assert!(!agg.is_complete());
}

// ---------------------------------------------------------------------------
// StreamSummary
// ---------------------------------------------------------------------------

#[test]
fn summary_empty_aggregator() {
    let agg = StreamAggregator::new();
    let s = agg.to_summary();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.text_length, 0);
    assert_eq!(s.tool_call_count, 0);
    assert!(!s.has_thinking);
    assert!(!s.has_errors);
    assert!(s.duration_ms.is_none());
}

#[test]
fn summary_reflects_aggregation_state() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("Hello"));
    agg.push(&tool_call("read", "t1", serde_json::json!({})));
    agg.push(&error_event("oops"));
    agg.push(&run_completed());

    let s = agg.to_summary();
    assert_eq!(s.total_events, 5);
    assert_eq!(s.text_length, 5); // "Hello"
    assert_eq!(s.tool_call_count, 1);
    assert!(!s.has_thinking);
    assert!(s.has_errors);
    // duration_ms is set since we have events
    assert!(s.duration_ms.is_some());
}

#[test]
fn summary_duration_is_some_after_multiple_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("a"));
    agg.push(&delta("b"));
    let s = agg.to_summary();
    assert!(s.duration_ms.is_some());
}

// ---------------------------------------------------------------------------
// Non-text events still counted
// ---------------------------------------------------------------------------

#[test]
fn file_changed_events_counted() {
    let mut agg = StreamAggregator::new();
    agg.push(&file_changed_event("src/main.rs"));
    assert_eq!(agg.event_count(), 1);
    assert_eq!(agg.text(), "");
}

#[test]
fn command_events_counted() {
    let mut agg = StreamAggregator::new();
    agg.push(&command_event("cargo test"));
    assert_eq!(agg.event_count(), 1);
    assert_eq!(agg.text(), "");
}

// ---------------------------------------------------------------------------
// Serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn tool_call_aggregate_serializes() {
    let tc = ToolCallAggregate {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"foo"}"#.to_string(),
        result: Some("contents".to_string()),
    };
    let json = serde_json::to_string(&tc).expect("serialize");
    let deser: ToolCallAggregate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.id, "tc1");
    assert_eq!(deser.name, "read_file");
    assert_eq!(deser.result.as_deref(), Some("contents"));
}

#[test]
fn stream_summary_serializes() {
    let s = StreamSummary {
        total_events: 42,
        text_length: 100,
        tool_call_count: 3,
        has_thinking: false,
        has_errors: true,
        duration_ms: Some(1234),
    };
    let json = serde_json::to_string(&s).expect("serialize");
    let deser: StreamSummary = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.total_events, 42);
    assert_eq!(deser.duration_ms, Some(1234));
    assert!(deser.has_errors);
}

// ---------------------------------------------------------------------------
// Full stream simulation
// ---------------------------------------------------------------------------

#[test]
fn full_stream_lifecycle() {
    let mut agg = StreamAggregator::new();

    agg.push(&run_started());
    agg.push(&delta("I'll read the file."));
    agg.push(&tool_call(
        "read_file",
        "tc1",
        serde_json::json!({"path": "main.rs"}),
    ));
    agg.push(&tool_result("tc1", serde_json::json!("fn main() {}")));
    agg.push(&delta(" The file contains a main function."));
    agg.push(&run_completed());

    assert!(agg.is_complete());
    assert_eq!(
        agg.text(),
        "I'll read the file. The file contains a main function."
    );
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(agg.errors().is_empty());
    assert_eq!(agg.event_count(), 6);

    let summary = agg.to_summary();
    assert_eq!(summary.tool_call_count, 1);
    assert!(!summary.has_errors);
}
