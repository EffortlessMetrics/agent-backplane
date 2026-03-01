// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for [`abp_core::filter::EventFilter`] applied to event streams.

use abp_core::filter::EventFilter;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;

// ── Helpers ─────────────────────────────────────────────────────────

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

/// Returns one representative event for every known [`AgentEventKind`] variant.
fn all_event_kinds() -> Vec<AgentEvent> {
    vec![
        event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        }),
        event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp"}),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tc-1".into()),
            output: serde_json::json!("contents"),
            is_error: false,
        }),
        event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        }),
        event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
        event(AgentEventKind::Warning {
            message: "heads up".into(),
        }),
        event(AgentEventKind::Error {
            message: "boom".into(),
        }),
    ]
}

/// All known serde tag names for [`AgentEventKind`].
const ALL_KIND_NAMES: &[&str] = &[
    "run_started",
    "run_completed",
    "assistant_delta",
    "assistant_message",
    "tool_call",
    "tool_result",
    "file_changed",
    "command_executed",
    "warning",
    "error",
];

/// Apply a filter to a slice of events and return those that pass.
fn filter_events<'a>(filter: &EventFilter, events: &'a [AgentEvent]) -> Vec<&'a AgentEvent> {
    events.iter().filter(|e| filter.matches(e)).collect()
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn filter_mixed_stream_include() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        event(AgentEventKind::Error {
            message: "oops".into(),
        }),
        event(AgentEventKind::Warning {
            message: "hmm".into(),
        }),
    ];
    let f = EventFilter::include_kinds(&["assistant_message", "error"]);
    let passed = filter_events(&f, &events);
    assert_eq!(passed.len(), 2);
    assert!(matches!(
        passed[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(passed[1].kind, AgentEventKind::Error { .. }));
}

#[test]
fn filter_mixed_stream_exclude() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        event(AgentEventKind::Error {
            message: "e".into(),
        }),
        event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let f = EventFilter::exclude_kinds(&["warning", "error"]);
    let passed = filter_events(&f, &events);
    assert_eq!(passed.len(), 2);
    assert!(matches!(passed[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        passed[1].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn multiple_include_kinds() {
    let f = EventFilter::include_kinds(&["tool_call", "tool_result", "file_changed"]);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    assert_eq!(passed.len(), 3);
    assert!(matches!(passed[0].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(passed[1].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(passed[2].kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn multiple_exclude_kinds() {
    let f =
        EventFilter::exclude_kinds(&["assistant_delta", "assistant_message", "warning", "error"]);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    // Should keep: run_started, run_completed, tool_call, tool_result, file_changed, command_executed
    assert_eq!(passed.len(), 6);
    for e in &passed {
        assert!(!matches!(
            e.kind,
            AgentEventKind::AssistantDelta { .. }
                | AgentEventKind::AssistantMessage { .. }
                | AgentEventKind::Warning { .. }
                | AgentEventKind::Error { .. }
        ));
    }
}

#[test]
fn filter_against_all_known_kinds_include() {
    let events = all_event_kinds();
    for name in ALL_KIND_NAMES {
        let f = EventFilter::include_kinds(&[name]);
        let passed = filter_events(&f, &events);
        assert_eq!(
            passed.len(),
            1,
            "include({name}) should match exactly one event from all_event_kinds()"
        );
    }
}

#[test]
fn filter_against_all_known_kinds_exclude() {
    let events = all_event_kinds();
    let total = events.len();
    for name in ALL_KIND_NAMES {
        let f = EventFilter::exclude_kinds(&[name]);
        let passed = filter_events(&f, &events);
        assert_eq!(
            passed.len(),
            total - 1,
            "exclude({name}) should reject exactly one event from all_event_kinds()"
        );
    }
}

#[test]
fn empty_include_passes_nothing() {
    let f = EventFilter::include_kinds(&[]);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    assert!(passed.is_empty(), "empty include list should pass nothing");
}

#[test]
fn empty_exclude_passes_everything() {
    let f = EventFilter::exclude_kinds(&[]);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    assert_eq!(
        passed.len(),
        events.len(),
        "empty exclude list should pass everything"
    );
}

#[test]
fn include_all_kinds_passes_everything() {
    let f = EventFilter::include_kinds(ALL_KIND_NAMES);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    assert_eq!(
        passed.len(),
        events.len(),
        "including all kinds should pass every event"
    );
}

#[test]
fn exclude_all_kinds_passes_nothing() {
    let f = EventFilter::exclude_kinds(ALL_KIND_NAMES);
    let events = all_event_kinds();
    let passed = filter_events(&f, &events);
    assert!(passed.is_empty(), "excluding all kinds should pass nothing");
}

#[test]
fn helper_all_event_kinds_covers_every_variant() {
    let events = all_event_kinds();
    assert_eq!(
        events.len(),
        ALL_KIND_NAMES.len(),
        "all_event_kinds() should produce one event per known kind"
    );
    // Verify each kind name is represented exactly once.
    for name in ALL_KIND_NAMES {
        let f = EventFilter::include_kinds(&[name]);
        let count = events.iter().filter(|e| f.matches(e)).count();
        assert_eq!(count, 1, "expected exactly one event for kind {name}");
    }
}
