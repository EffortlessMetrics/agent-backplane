// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for [`abp_core::stream::EventStream`].

use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{Duration as ChronoDuration, Utc};

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn event_at(kind: AgentEventKind, offset_secs: i64) -> AgentEvent {
    let base = Utc::now();
    AgentEvent {
        ts: base + ChronoDuration::seconds(offset_secs),
        kind,
        ext: None,
    }
}

fn sample_events() -> Vec<AgentEvent> {
    vec![
        event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        event_at(
            AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            1,
        ),
        event_at(
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            2,
        ),
        event_at(
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: None,
                output: serde_json::json!("content"),
                is_error: false,
            },
            3,
        ),
        event_at(
            AgentEventKind::Warning {
                message: "caution".into(),
            },
            4,
        ),
        event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            5,
        ),
    ]
}

// --- new / is_empty / len ---

#[test]
fn new_empty_stream() {
    let s = EventStream::new(vec![]);
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
}

#[test]
fn new_with_events() {
    let s = EventStream::new(sample_events());
    assert!(!s.is_empty());
    assert_eq!(s.len(), 6);
}

// --- by_kind ---

#[test]
fn by_kind_returns_matching_events() {
    let s = EventStream::new(sample_events());
    let filtered = s.by_kind("tool_call");
    assert_eq!(filtered.len(), 1);
}

#[test]
fn by_kind_case_insensitive() {
    let s = EventStream::new(sample_events());
    let filtered = s.by_kind("TOOL_CALL");
    assert_eq!(filtered.len(), 1);
}

#[test]
fn by_kind_no_match_returns_empty() {
    let s = EventStream::new(sample_events());
    let filtered = s.by_kind("nonexistent_kind");
    assert!(filtered.is_empty());
}

// --- filter ---

#[test]
fn filter_with_include() {
    let s = EventStream::new(sample_events());
    let f = EventFilter::include_kinds(&["run_started", "run_completed"]);
    let filtered = s.filter(&f);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filter_with_exclude() {
    let s = EventStream::new(sample_events());
    let f = EventFilter::exclude_kinds(&["warning"]);
    let filtered = s.filter(&f);
    assert_eq!(filtered.len(), 5);
}

// --- count_by_kind ---

#[test]
fn count_by_kind_counts_each_variant() {
    let s = EventStream::new(sample_events());
    let counts = s.count_by_kind();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_message"), Some(&1));
    assert_eq!(counts.get("tool_call"), Some(&1));
    assert_eq!(counts.get("tool_result"), Some(&1));
    assert_eq!(counts.get("warning"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
    assert_eq!(counts.len(), 6);
}

#[test]
fn count_by_kind_empty_stream() {
    let s = EventStream::new(vec![]);
    let counts = s.count_by_kind();
    assert!(counts.is_empty());
}

// --- first_of_kind / last_of_kind ---

#[test]
fn first_of_kind_returns_first_match() {
    let events = vec![
        event(AgentEventKind::Warning {
            message: "first".into(),
        }),
        event(AgentEventKind::Warning {
            message: "second".into(),
        }),
    ];
    let s = EventStream::new(events);
    let first = s.first_of_kind("warning").unwrap();
    match &first.kind {
        AgentEventKind::Warning { message } => assert_eq!(message, "first"),
        _ => panic!("expected Warning"),
    }
}

#[test]
fn last_of_kind_returns_last_match() {
    let events = vec![
        event(AgentEventKind::Warning {
            message: "first".into(),
        }),
        event(AgentEventKind::Warning {
            message: "second".into(),
        }),
    ];
    let s = EventStream::new(events);
    let last = s.last_of_kind("warning").unwrap();
    match &last.kind {
        AgentEventKind::Warning { message } => assert_eq!(message, "second"),
        _ => panic!("expected Warning"),
    }
}

#[test]
fn first_of_kind_returns_none_when_absent() {
    let s = EventStream::new(sample_events());
    assert!(s.first_of_kind("file_changed").is_none());
}

#[test]
fn last_of_kind_returns_none_when_absent() {
    let s = EventStream::new(sample_events());
    assert!(s.last_of_kind("file_changed").is_none());
}

// --- duration ---

#[test]
fn duration_with_multiple_events() {
    let s = EventStream::new(sample_events());
    let d = s.duration().unwrap();
    assert!(d.as_secs() >= 4);
}

#[test]
fn duration_single_event_returns_none() {
    let s = EventStream::new(vec![event(AgentEventKind::RunStarted {
        message: "go".into(),
    })]);
    assert!(s.duration().is_none());
}

#[test]
fn duration_empty_returns_none() {
    let s = EventStream::new(vec![]);
    assert!(s.duration().is_none());
}

// --- iter / IntoIterator ---

#[test]
fn iter_yields_all_events() {
    let s = EventStream::new(sample_events());
    assert_eq!(s.iter().count(), 6);
}

#[test]
fn into_iterator_owned() {
    let s = EventStream::new(sample_events());
    let collected: Vec<AgentEvent> = s.into_iter().collect();
    assert_eq!(collected.len(), 6);
}

#[test]
fn into_iterator_ref() {
    let s = EventStream::new(sample_events());
    let count = (&s).into_iter().count();
    assert_eq!(count, 6);
}

// --- chained combinators ---

#[test]
fn chained_by_kind_and_len() {
    let events = vec![
        event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        event(AgentEventKind::Error {
            message: "fail".into(),
        }),
    ];
    let s = EventStream::new(events);
    let tool_calls = s.by_kind("tool_call");
    assert_eq!(tool_calls.len(), 2);
    assert!(!tool_calls.is_empty());
}
