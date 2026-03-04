#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive event pipeline tests covering all AgentEventKind variants
//! through creation, serialization, deserialization, filtering, aggregation,
//! ordering, metadata propagation, and stream recording/replay.

use abp_core::ext::AgentEventExt;
use abp_core::{AgentEvent, AgentEventKind};
use abp_error::ErrorCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// =========================================================================
// Helpers
// =========================================================================

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn fixed_ts(offset_ms: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc()
        + Duration::milliseconds(offset_ms)
}

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: now(),
        kind,
        ext: None,
    }
}

fn event_at(kind: AgentEventKind, offset_ms: i64) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(offset_ms),
        kind,
        ext: None,
    }
}

fn event_with_ext(kind: AgentEventKind, ext: BTreeMap<String, Value>) -> AgentEvent {
    AgentEvent {
        ts: now(),
        kind,
        ext: Some(ext),
    }
}

fn roundtrip(event: &AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(event).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn all_variant_events() -> Vec<AgentEvent> {
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
            text: "full".into(),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        }),
        event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added function".into(),
        }),
        event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        }),
        event(AgentEventKind::Warning {
            message: "rate limit approaching".into(),
        }),
        event(AgentEventKind::Error {
            message: "backend timeout".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        }),
    ]
}

fn type_name_of(kind: &AgentEventKind) -> &'static str {
    match kind {
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

// =========================================================================
// 1. Event creation — one test per variant
// =========================================================================

#[test]
fn create_run_started() {
    let e = event(AgentEventKind::RunStarted {
        message: "hello".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn create_run_completed() {
    let e = event(AgentEventKind::RunCompleted {
        message: "bye".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn create_assistant_delta() {
    let e = event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn create_assistant_message() {
    let e = event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn create_tool_call() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    });
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn create_tool_result() {
    let e = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("id1".into()),
        output: json!("ok"),
        is_error: false,
    });
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn create_file_changed() {
    let e = event(AgentEventKind::FileChanged {
        path: "a.txt".into(),
        summary: "created".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn create_command_executed() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "echo hi".into(),
        exit_code: Some(0),
        output_preview: Some("hi".into()),
    });
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn create_warning() {
    let e = event(AgentEventKind::Warning {
        message: "careful".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn create_error() {
    let e = event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(matches!(e.kind, AgentEventKind::Error { .. }));
}

#[test]
fn create_error_with_code() {
    let e = event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    });
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("wrong variant");
    }
}

// =========================================================================
// 2. Serialization roundtrip — one test per variant
// =========================================================================

#[test]
fn roundtrip_run_started() {
    let e = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert_eq!(message, "go");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn roundtrip_run_completed() {
    let e = event(AgentEventKind::RunCompleted {
        message: "fin".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunCompleted { message } = &rt.kind {
        assert_eq!(message, "fin");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_assistant_delta() {
    let e = event(AgentEventKind::AssistantDelta { text: "abc".into() });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "abc");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_assistant_message() {
    let e = event(AgentEventKind::AssistantMessage {
        text: "whole".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "whole");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_tool_call() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("t1".into()),
        parent_tool_use_id: Some("p1".into()),
        input: json!({"x": 42}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        parent_tool_use_id,
        input,
    } = &rt.kind
    {
        assert_eq!(tool_name, "read");
        assert_eq!(tool_use_id.as_deref(), Some("t1"));
        assert_eq!(parent_tool_use_id.as_deref(), Some("p1"));
        assert_eq!(input, &json!({"x": 42}));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_tool_result() {
    let e = event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("t1".into()),
        output: json!({"data": [1, 2, 3]}),
        is_error: true,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult {
        tool_name,
        tool_use_id,
        output,
        is_error,
    } = &rt.kind
    {
        assert_eq!(tool_name, "read");
        assert_eq!(tool_use_id.as_deref(), Some("t1"));
        assert_eq!(output, &json!({"data": [1, 2, 3]}));
        assert!(*is_error);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_file_changed() {
    let e = event(AgentEventKind::FileChanged {
        path: "foo/bar.rs".into(),
        summary: "modified".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert_eq!(path, "foo/bar.rs");
        assert_eq!(summary, "modified");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_command_executed() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "ls -la".into(),
        exit_code: Some(1),
        output_preview: Some("err".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &rt.kind
    {
        assert_eq!(command, "ls -la");
        assert_eq!(*exit_code, Some(1));
        assert_eq!(output_preview.as_deref(), Some("err"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_warning() {
    let e = event(AgentEventKind::Warning {
        message: "warn me".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert_eq!(message, "warn me");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_error() {
    let e = event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: Some(ErrorCode::Internal),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &rt.kind
    {
        assert_eq!(message, "fail");
        assert_eq!(*error_code, Some(ErrorCode::Internal));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn roundtrip_error_without_code() {
    let e = event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &rt.kind
    {
        assert_eq!(message, "oops");
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

// =========================================================================
// 3. JSON "type" discriminator field
// =========================================================================

#[test]
fn json_type_field_run_started() {
    let e = event(AgentEventKind::RunStarted {
        message: "s".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_started");
}

#[test]
fn json_type_field_run_completed() {
    let e = event(AgentEventKind::RunCompleted {
        message: "d".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_completed");
}

#[test]
fn json_type_field_assistant_delta() {
    let e = event(AgentEventKind::AssistantDelta { text: "x".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "assistant_delta");
}

#[test]
fn json_type_field_assistant_message() {
    let e = event(AgentEventKind::AssistantMessage { text: "y".into() });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "assistant_message");
}

#[test]
fn json_type_field_tool_call() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "t".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_call");
}

#[test]
fn json_type_field_tool_result() {
    let e = event(AgentEventKind::ToolResult {
        tool_name: "t".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn json_type_field_file_changed() {
    let e = event(AgentEventKind::FileChanged {
        path: "a".into(),
        summary: "b".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "file_changed");
}

#[test]
fn json_type_field_command_executed() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "c".into(),
        exit_code: None,
        output_preview: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "command_executed");
}

#[test]
fn json_type_field_warning() {
    let e = event(AgentEventKind::Warning {
        message: "w".into(),
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "warning");
}

#[test]
fn json_type_field_error() {
    let e = event(AgentEventKind::Error {
        message: "e".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "error");
}

// =========================================================================
// 4. All variants produce distinct type discriminators
// =========================================================================

#[test]
fn all_variants_have_unique_type_discriminators() {
    let events = all_variant_events();
    let mut types: Vec<String> = events
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            v["type"].as_str().unwrap().to_string()
        })
        .collect();
    let count = types.len();
    types.sort();
    types.dedup();
    assert_eq!(types.len(), count, "duplicate type discriminators found");
}

#[test]
fn all_10_variants_covered() {
    let events = all_variant_events();
    assert_eq!(
        events.len(),
        10,
        "expected exactly 10 AgentEventKind variants"
    );
}

// =========================================================================
// 5. Timestamp handling
// =========================================================================

#[test]
fn timestamp_preserved_through_roundtrip() {
    let ts = fixed_ts(12345);
    let e = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "t".into(),
        },
        ext: None,
    };
    let rt = roundtrip(&e);
    assert_eq!(rt.ts, ts);
}

#[test]
fn timestamp_serialized_as_rfc3339() {
    let e = event_at(
        AgentEventKind::RunStarted {
            message: "t".into(),
        },
        0,
    );
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("2025-01-01T00:00:00"));
}

#[test]
fn timestamp_ordering_across_events() {
    let e1 = event_at(
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        0,
    );
    let e2 = event_at(AgentEventKind::AssistantDelta { text: "b".into() }, 100);
    let e3 = event_at(
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        200,
    );
    assert!(e1.ts < e2.ts);
    assert!(e2.ts < e3.ts);
}

#[test]
fn timestamp_sub_millisecond_preserved() {
    let ts = DateTime::parse_from_rfc3339("2025-06-15T12:30:45.123456789Z")
        .unwrap()
        .to_utc();
    let e = AgentEvent {
        ts,
        kind: AgentEventKind::Warning {
            message: "ns".into(),
        },
        ext: None,
    };
    let rt = roundtrip(&e);
    // chrono + serde_json may truncate to microseconds; verify at least ms
    let diff = (rt.ts - ts).num_milliseconds().abs();
    assert!(diff < 1, "timestamp drift: {diff}ms");
}

#[test]
fn timestamp_negative_offset_ordering() {
    let e1 = event_at(
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        -1000,
    );
    let e2 = event_at(
        AgentEventKind::RunStarted {
            message: "b".into(),
        },
        0,
    );
    assert!(e1.ts < e2.ts);
}

// =========================================================================
// 6. Event ordering and sequence numbering
// =========================================================================

#[test]
fn events_sortable_by_timestamp() {
    let mut events = vec![
        event_at(
            AgentEventKind::RunCompleted {
                message: "end".into(),
            },
            300,
        ),
        event_at(
            AgentEventKind::RunStarted {
                message: "begin".into(),
            },
            0,
        ),
        event_at(AgentEventKind::AssistantDelta { text: "mid".into() }, 150),
    ];
    events.sort_by_key(|e| e.ts);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn sequence_numbering_via_enumerate() {
    let events = all_variant_events();
    let numbered: Vec<(usize, &AgentEvent)> = events.iter().enumerate().collect();
    assert_eq!(numbered.len(), 10);
    assert_eq!(numbered[0].0, 0);
    assert_eq!(numbered[9].0, 9);
}

#[test]
fn events_stable_sort_preserves_insertion_order_for_same_ts() {
    let ts = fixed_ts(0);
    let mut events: Vec<(usize, AgentEvent)> = (0..5)
        .map(|i| {
            (
                i,
                AgentEvent {
                    ts,
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("d{i}"),
                    },
                    ext: None,
                },
            )
        })
        .collect();
    events.sort_by_key(|(_, e)| e.ts);
    for (i, (orig_idx, _)) in events.iter().enumerate() {
        assert_eq!(*orig_idx, i, "stable sort should preserve original order");
    }
}

#[test]
fn pipeline_run_started_first_run_completed_last() {
    let events = vec![
        event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        event_at(AgentEventKind::AssistantDelta { text: "a".into() }, 10),
        event_at(
            AgentEventKind::ToolCall {
                tool_name: "t".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!(null),
            },
            20,
        ),
        event_at(
            AgentEventKind::ToolResult {
                tool_name: "t".into(),
                tool_use_id: None,
                output: json!(null),
                is_error: false,
            },
            30,
        ),
        event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            40,
        ),
    ];
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// =========================================================================
// 7. Event filtering by kind
// =========================================================================

#[test]
fn filter_only_tool_calls() {
    let events = all_variant_events();
    let tools: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert_eq!(tools.len(), 1);
}

#[test]
fn filter_only_tool_results() {
    let events = all_variant_events();
    let results: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_only_assistant_deltas() {
    let events = all_variant_events();
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 1);
}

#[test]
fn filter_only_file_changes() {
    let events = all_variant_events();
    let changes: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::FileChanged { .. }))
        .collect();
    assert_eq!(changes.len(), 1);
}

#[test]
fn filter_only_commands() {
    let events = all_variant_events();
    let cmds: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::CommandExecuted { .. }))
        .collect();
    assert_eq!(cmds.len(), 1);
}

#[test]
fn filter_only_warnings() {
    let events = all_variant_events();
    let warns: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Warning { .. }))
        .collect();
    assert_eq!(warns.len(), 1);
}

#[test]
fn filter_only_errors() {
    let events = all_variant_events();
    let errs: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .collect();
    assert_eq!(errs.len(), 1);
}

#[test]
fn filter_text_events() {
    let events = all_variant_events();
    let text_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::AssistantDelta { .. } | AgentEventKind::AssistantMessage { .. }
            )
        })
        .collect();
    assert_eq!(text_events.len(), 2);
}

#[test]
fn filter_lifecycle_events() {
    let events = all_variant_events();
    let lifecycle: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::RunStarted { .. } | AgentEventKind::RunCompleted { .. }
            )
        })
        .collect();
    assert_eq!(lifecycle.len(), 2);
}

#[test]
fn filter_side_effects() {
    let events = all_variant_events();
    let side_effects: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::FileChanged { .. } | AgentEventKind::CommandExecuted { .. }
            )
        })
        .collect();
    assert_eq!(side_effects.len(), 2);
}

#[test]
fn filter_diagnostic_events() {
    let events = all_variant_events();
    let diag: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                AgentEventKind::Warning { .. } | AgentEventKind::Error { .. }
            )
        })
        .collect();
    assert_eq!(diag.len(), 2);
}

#[test]
fn filter_by_type_name_string() {
    let events = all_variant_events();
    let expected_names = [
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
    for name in &expected_names {
        let found: Vec<_> = events
            .iter()
            .filter(|e| type_name_of(&e.kind) == *name)
            .collect();
        assert_eq!(found.len(), 1, "expected exactly 1 event of type {name}");
    }
}

// =========================================================================
// 8. AgentEventExt trait
// =========================================================================

#[test]
fn ext_is_tool_call_true_for_tool_call() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "x".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    assert!(e.is_tool_call());
}

#[test]
fn ext_is_tool_call_false_for_others() {
    let e = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    assert!(!e.is_tool_call());
}

#[test]
fn ext_is_terminal_true_for_run_completed() {
    let e = event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(e.is_terminal());
}

#[test]
fn ext_is_terminal_false_for_run_started() {
    let e = event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    assert!(!e.is_terminal());
}

#[test]
fn ext_text_content_from_delta() {
    let e = event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    });
    assert_eq!(e.text_content(), Some("hello"));
}

#[test]
fn ext_text_content_from_message() {
    let e = event(AgentEventKind::AssistantMessage {
        text: "world".into(),
    });
    assert_eq!(e.text_content(), Some("world"));
}

#[test]
fn ext_text_content_none_for_tool_call() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "x".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    assert_eq!(e.text_content(), None);
}

#[test]
fn ext_text_content_none_for_error() {
    let e = event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    assert_eq!(e.text_content(), None);
}

#[test]
fn ext_text_content_none_for_warning() {
    let e = event(AgentEventKind::Warning {
        message: "w".into(),
    });
    assert_eq!(e.text_content(), None);
}

#[test]
fn ext_text_content_none_for_file_changed() {
    let e = event(AgentEventKind::FileChanged {
        path: "p".into(),
        summary: "s".into(),
    });
    assert_eq!(e.text_content(), None);
}

#[test]
fn ext_text_content_none_for_command_executed() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "c".into(),
        exit_code: None,
        output_preview: None,
    });
    assert_eq!(e.text_content(), None);
}

// =========================================================================
// 9. Delta aggregation
// =========================================================================

#[test]
fn aggregate_deltas_into_full_message() {
    let deltas = vec![
        event(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        }),
        event(AgentEventKind::AssistantDelta { text: ", ".into() }),
        event(AgentEventKind::AssistantDelta {
            text: "world!".into(),
        }),
    ];
    let full: String = deltas.iter().filter_map(|e| e.text_content()).collect();
    assert_eq!(full, "Hello, world!");
}

#[test]
fn aggregate_mixed_stream_extracts_only_text() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        event(AgentEventKind::AssistantDelta { text: "A".into() }),
        event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::AssistantDelta { text: "B".into() }),
        event(AgentEventKind::AssistantMessage { text: "C".into() }),
    ];
    let text: String = events.iter().filter_map(|e| e.text_content()).collect();
    assert_eq!(text, "ABC");
}

#[test]
fn aggregate_empty_deltas() {
    let deltas: Vec<AgentEvent> = vec![
        event(AgentEventKind::AssistantDelta { text: "".into() }),
        event(AgentEventKind::AssistantDelta { text: "".into() }),
    ];
    let full: String = deltas.iter().filter_map(|e| e.text_content()).collect();
    assert_eq!(full, "");
}

#[test]
fn aggregate_single_delta() {
    let deltas = vec![event(AgentEventKind::AssistantDelta {
        text: "only".into(),
    })];
    let full: String = deltas.iter().filter_map(|e| e.text_content()).collect();
    assert_eq!(full, "only");
}

#[test]
fn aggregate_unicode_deltas() {
    let deltas = vec![
        event(AgentEventKind::AssistantDelta {
            text: "こん".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "にちは".into(),
        }),
    ];
    let full: String = deltas.iter().filter_map(|e| e.text_content()).collect();
    assert_eq!(full, "こんにちは");
}

#[test]
fn aggregate_large_delta_stream() {
    let deltas: Vec<AgentEvent> = (0..1000)
        .map(|i| {
            event(AgentEventKind::AssistantDelta {
                text: format!("{i}"),
            })
        })
        .collect();
    let full: String = deltas.iter().filter_map(|e| e.text_content()).collect();
    assert!(full.contains("0"));
    assert!(full.contains("999"));
}

// =========================================================================
// 10. Event stream recording and replay
// =========================================================================

#[test]
fn record_and_replay_full_stream() {
    let stream = all_variant_events();
    let recorded: Vec<String> = stream
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    let replayed: Vec<AgentEvent> = recorded
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replayed.len(), stream.len());
    for (orig, replay) in stream.iter().zip(replayed.iter()) {
        assert_eq!(type_name_of(&orig.kind), type_name_of(&replay.kind));
        assert_eq!(orig.ts, replay.ts);
    }
}

#[test]
fn record_as_jsonl() {
    let stream = all_variant_events();
    let jsonl: String = stream
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 10);
    for line in &lines {
        let _: AgentEvent = serde_json::from_str(line).unwrap();
    }
}

#[test]
fn replay_preserves_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"sdk": "anthropic"}));
    let e = event_with_ext(
        AgentEventKind::AssistantMessage { text: "hi".into() },
        ext.clone(),
    );
    let json = serde_json::to_string(&e).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.ext.unwrap()["raw_message"], json!({"sdk": "anthropic"}));
}

#[test]
fn replay_empty_stream() {
    let stream: Vec<AgentEvent> = vec![];
    let recorded: Vec<String> = stream
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    let replayed: Vec<AgentEvent> = recorded
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(replayed.is_empty());
}

#[test]
fn replay_single_event_stream() {
    let stream = vec![event(AgentEventKind::RunStarted {
        message: "solo".into(),
    })];
    let json = serde_json::to_string(&stream[0]).unwrap();
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert_eq!(message, "solo");
    } else {
        panic!("wrong variant");
    }
}

// =========================================================================
// 11. Metadata / ext propagation
// =========================================================================

#[test]
fn ext_field_none_by_default() {
    let e = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    assert!(e.ext.is_none());
}

#[test]
fn ext_field_omitted_in_json_when_none() {
    let e = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("ext"));
    assert!(!json.contains("raw_message"));
}

#[test]
fn ext_field_present_in_json_when_some() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!("hello"));
    let e = event_with_ext(
        AgentEventKind::Warning {
            message: "w".into(),
        },
        ext,
    );
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("raw_message"));
}

#[test]
fn ext_multiple_keys_preserved() {
    let mut ext = BTreeMap::new();
    ext.insert("key_a".into(), json!(1));
    ext.insert("key_b".into(), json!("two"));
    ext.insert("key_c".into(), json!([3, 4]));
    let e = event_with_ext(AgentEventKind::AssistantDelta { text: "t".into() }, ext);
    let rt = roundtrip(&e);
    let ext_rt = rt.ext.unwrap();
    assert_eq!(ext_rt["key_a"], json!(1));
    assert_eq!(ext_rt["key_b"], json!("two"));
    assert_eq!(ext_rt["key_c"], json!([3, 4]));
}

#[test]
fn ext_nested_objects_preserved() {
    let mut ext = BTreeMap::new();
    ext.insert("deep".into(), json!({"a": {"b": {"c": 42}}}));
    let e = event_with_ext(
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        },
        ext,
    );
    let rt = roundtrip(&e);
    let ext_rt = rt.ext.unwrap();
    assert_eq!(ext_rt["deep"]["a"]["b"]["c"], 42);
}

#[test]
fn ext_propagated_through_all_variants() {
    let mut ext = BTreeMap::new();
    ext.insert("trace_id".into(), json!("abc-123"));
    for ev in all_variant_events() {
        let e = AgentEvent {
            ts: ev.ts,
            kind: ev.kind.clone(),
            ext: Some(ext.clone()),
        };
        let rt = roundtrip(&e);
        assert_eq!(
            rt.ext.as_ref().unwrap()["trace_id"],
            json!("abc-123"),
            "ext lost for variant {}",
            type_name_of(&ev.kind)
        );
    }
}

// =========================================================================
// 12. ErrorCode integration
// =========================================================================

#[test]
fn error_code_as_str_is_snake_case() {
    let code = ErrorCode::BackendTimeout;
    assert_eq!(code.as_str(), "backend_timeout");
}

#[test]
fn error_code_serializes_as_snake_case() {
    let code = ErrorCode::ProtocolInvalidEnvelope;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, "\"protocol_invalid_envelope\"");
}

#[test]
fn error_event_with_code_roundtrip() {
    let e = event(AgentEventKind::Error {
        message: "test".into(),
        error_code: Some(ErrorCode::PolicyDenied),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error {
        error_code,
        message,
        ..
    } = &rt.kind
    {
        assert_eq!(*error_code, Some(ErrorCode::PolicyDenied));
        assert_eq!(message, "test");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_code_none_not_serialized() {
    let e = event(AgentEventKind::Error {
        message: "no code".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn error_code_present_when_some() {
    let e = event(AgentEventKind::Error {
        message: "with code".into(),
        error_code: Some(ErrorCode::Internal),
    });
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("\"error_code\""));
    assert!(json.contains("\"internal\""));
}

#[test]
fn error_code_various_categories() {
    let codes = vec![
        (
            ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (ErrorCode::BackendTimeout, "backend_timeout"),
        (ErrorCode::ExecutionToolFailed, "execution_tool_failed"),
        (
            ErrorCode::ContractVersionMismatch,
            "contract_version_mismatch",
        ),
        (ErrorCode::PolicyDenied, "policy_denied"),
        (ErrorCode::WorkspaceInitFailed, "workspace_init_failed"),
        (ErrorCode::Internal, "internal"),
    ];
    for (code, expected_str) in codes {
        assert_eq!(code.as_str(), expected_str);
        let e = event(AgentEventKind::Error {
            message: "x".into(),
            error_code: Some(code),
        });
        let rt = roundtrip(&e);
        if let AgentEventKind::Error { error_code, .. } = &rt.kind {
            assert_eq!(*error_code, Some(code));
        }
    }
}

// =========================================================================
// 13. ToolCall / ToolResult field variations
// =========================================================================

#[test]
fn tool_call_with_all_optional_fields() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: Some("parent_1".into()),
        input: json!({"cmd": "ls", "args": ["-la"]}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_use_id,
        parent_tool_use_id,
        ..
    } = &rt.kind
    {
        assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
        assert_eq!(parent_tool_use_id.as_deref(), Some("parent_1"));
    }
}

#[test]
fn tool_call_with_no_optional_fields() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_use_id,
        parent_tool_use_id,
        ..
    } = &rt.kind
    {
        assert!(tool_use_id.is_none());
        assert!(parent_tool_use_id.is_none());
    }
}

#[test]
fn tool_result_error_flag_true() {
    let e = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!({"error": "not found"}),
        is_error: true,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { is_error, .. } = &rt.kind {
        assert!(*is_error);
    }
}

#[test]
fn tool_result_error_flag_false() {
    let e = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("success"),
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { is_error, .. } = &rt.kind {
        assert!(!*is_error);
    }
}

#[test]
fn tool_call_complex_input() {
    let input = json!({
        "command": "find",
        "args": ["-name", "*.rs"],
        "options": {"recursive": true, "max_depth": 5},
        "tags": [1, 2, 3]
    });
    let e = event(AgentEventKind::ToolCall {
        tool_name: "find".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: input.clone(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        input: rt_input, ..
    } = &rt.kind
    {
        assert_eq!(*rt_input, input);
    }
}

#[test]
fn tool_result_complex_output() {
    let output = json!({
        "files": ["a.rs", "b.rs"],
        "count": 2,
        "metadata": {"elapsed_ms": 42}
    });
    let e = event(AgentEventKind::ToolResult {
        tool_name: "find".into(),
        tool_use_id: None,
        output: output.clone(),
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult {
        output: rt_output, ..
    } = &rt.kind
    {
        assert_eq!(*rt_output, output);
    }
}

// =========================================================================
// 14. CommandExecuted field variations
// =========================================================================

#[test]
fn command_executed_with_all_fields() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(0),
        output_preview: Some("Compiling...".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &rt.kind
    {
        assert_eq!(command, "cargo build");
        assert_eq!(*exit_code, Some(0));
        assert_eq!(output_preview.as_deref(), Some("Compiling..."));
    }
}

#[test]
fn command_executed_no_optional_fields() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "echo".into(),
        exit_code: None,
        output_preview: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted {
        exit_code,
        output_preview,
        ..
    } = &rt.kind
    {
        assert!(exit_code.is_none());
        assert!(output_preview.is_none());
    }
}

#[test]
fn command_executed_nonzero_exit() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "false".into(),
        exit_code: Some(1),
        output_preview: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &rt.kind {
        assert_eq!(*exit_code, Some(1));
    }
}

#[test]
fn command_executed_negative_exit() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "killed".into(),
        exit_code: Some(-9),
        output_preview: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &rt.kind {
        assert_eq!(*exit_code, Some(-9));
    }
}

// =========================================================================
// 15. Deserialization from raw JSON
// =========================================================================

#[test]
fn deserialize_run_started_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"begin"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn deserialize_run_completed_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"run_completed","message":"end"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn deserialize_assistant_delta_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"tok"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn deserialize_assistant_message_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"full"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn deserialize_tool_call_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"tool_call","tool_name":"bash","input":{},"tool_use_id":"t1"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn deserialize_tool_result_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"tool_result","tool_name":"bash","output":"ok","is_error":false}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn deserialize_file_changed_from_json() {
    let json =
        r#"{"ts":"2025-01-01T00:00:00Z","type":"file_changed","path":"a.rs","summary":"new"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn deserialize_command_executed_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"command_executed","command":"ls","exit_code":0,"output_preview":"files"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn deserialize_warning_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"careful"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn deserialize_error_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"error","message":"fail"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert!(error_code.is_none());
    }
}

#[test]
fn deserialize_error_with_code_from_json() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"error","message":"fail","error_code":"backend_timeout"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    }
}

// =========================================================================
// 16. Invalid / edge-case deserialization
// =========================================================================

#[test]
fn deserialize_unknown_type_fails() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"unknown_variant","data":42}"#;
    let result: Result<AgentEvent, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_missing_type_fails() {
    let json = r#"{"ts":"2025-01-01T00:00:00Z","message":"hello"}"#;
    let result: Result<AgentEvent, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_missing_ts_fails() {
    let json = r#"{"type":"run_started","message":"hello"}"#;
    let result: Result<AgentEvent, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_missing_required_field_fails() {
    // tool_call without tool_name
    let json = r#"{"ts":"2025-01-01T00:00:00Z","type":"tool_call","input":{}}"#;
    let result: Result<AgentEvent, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// =========================================================================
// 17. Clone and Debug
// =========================================================================

#[test]
fn all_variants_cloneable() {
    for e in all_variant_events() {
        let cloned = e.clone();
        assert_eq!(type_name_of(&cloned.kind), type_name_of(&e.kind));
        assert_eq!(cloned.ts, e.ts);
    }
}

#[test]
fn all_variants_debuggable() {
    for e in all_variant_events() {
        let dbg = format!("{:?}", e);
        assert!(!dbg.is_empty());
    }
}

// =========================================================================
// 18. Pipeline simulation: full lifecycle
// =========================================================================

#[test]
fn full_lifecycle_pipeline() {
    let events = vec![
        event_at(
            AgentEventKind::RunStarted {
                message: "Starting task".into(),
            },
            0,
        ),
        event_at(
            AgentEventKind::AssistantDelta {
                text: "Let me ".into(),
            },
            10,
        ),
        event_at(
            AgentEventKind::AssistantDelta {
                text: "think...".into(),
            },
            20,
        ),
        event_at(
            AgentEventKind::AssistantMessage {
                text: "I'll read the file.".into(),
            },
            30,
        ),
        event_at(
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            40,
        ),
        event_at(
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                output: json!({"content": "fn main() {}"}),
                is_error: false,
            },
            50,
        ),
        event_at(
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added logging".into(),
            },
            60,
        ),
        event_at(
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok".into()),
            },
            70,
        ),
        event_at(
            AgentEventKind::AssistantMessage {
                text: "Done!".into(),
            },
            80,
        ),
        event_at(
            AgentEventKind::RunCompleted {
                message: "Task completed".into(),
            },
            90,
        ),
    ];

    // Verify ordering
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }

    // Verify lifecycle bookends
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));

    // Verify terminal detection
    let terminal_count = events.iter().filter(|e| e.is_terminal()).count();
    assert_eq!(terminal_count, 1);

    // Verify tool call count
    let tool_calls = events.iter().filter(|e| e.is_tool_call()).count();
    assert_eq!(tool_calls, 1);

    // Verify text aggregation
    let text: String = events.iter().filter_map(|e| e.text_content()).collect();
    assert!(text.contains("Let me "));
    assert!(text.contains("think..."));
    assert!(text.contains("Done!"));
}

#[test]
fn pipeline_with_errors_and_warnings() {
    let events = vec![
        event_at(
            AgentEventKind::RunStarted {
                message: "begin".into(),
            },
            0,
        ),
        event_at(
            AgentEventKind::Warning {
                message: "low memory".into(),
            },
            10,
        ),
        event_at(
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"cmd": "rm -rf /"}),
            },
            20,
        ),
        event_at(
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_1".into()),
                output: json!({"error": "permission denied"}),
                is_error: true,
            },
            30,
        ),
        event_at(
            AgentEventKind::Error {
                message: "critical failure".into(),
                error_code: Some(ErrorCode::ExecutionPermissionDenied),
            },
            40,
        ),
        event_at(
            AgentEventKind::RunCompleted {
                message: "failed".into(),
            },
            50,
        ),
    ];

    let warnings: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Warning { .. }))
        .collect();
    assert_eq!(warnings.len(), 1);

    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .collect();
    assert_eq!(errors.len(), 1);

    let failed_tools: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { is_error: true, .. }))
        .collect();
    assert_eq!(failed_tools.len(), 1);
}

#[test]
fn pipeline_multiple_tool_cycles() {
    let mut events = vec![event_at(
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        0,
    )];

    for i in 0..5 {
        let offset = (i + 1) * 10;
        events.push(event_at(
            AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu_{i}")),
                parent_tool_use_id: None,
                input: json!({"iter": i}),
            },
            offset as i64,
        ));
        events.push(event_at(
            AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu_{i}")),
                output: json!({"result": i}),
                is_error: false,
            },
            (offset + 5) as i64,
        ));
    }

    events.push(event_at(
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        100,
    ));

    let calls = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .count();
    let results = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .count();
    assert_eq!(calls, 5);
    assert_eq!(results, 5);
}

#[test]
fn pipeline_nested_tool_calls() {
    let events = vec![
        event(AgentEventKind::ToolCall {
            tool_name: "orchestrator".into(),
            tool_use_id: Some("parent".into()),
            parent_tool_use_id: None,
            input: json!({"action": "plan"}),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "sub_tool".into(),
            tool_use_id: Some("child".into()),
            parent_tool_use_id: Some("parent".into()),
            input: json!({"action": "execute"}),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "sub_tool".into(),
            tool_use_id: Some("child".into()),
            output: json!("sub_result"),
            is_error: false,
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "orchestrator".into(),
            tool_use_id: Some("parent".into()),
            output: json!("parent_result"),
            is_error: false,
        }),
    ];

    // Verify parent-child relationship
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &events[1].kind
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("parent"));
    }
}

// =========================================================================
// 19. Empty / edge-case strings
// =========================================================================

#[test]
fn empty_message_run_started() {
    let e = event(AgentEventKind::RunStarted { message: "".into() });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert_eq!(message, "");
    }
}

#[test]
fn empty_text_assistant_delta() {
    let e = event(AgentEventKind::AssistantDelta { text: "".into() });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "");
    }
}

#[test]
fn unicode_in_all_string_fields() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "工具".into(),
        tool_use_id: Some("标识".into()),
        parent_tool_use_id: Some("父级".into()),
        input: json!({"路径": "文件.rs"}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        ..
    } = &rt.kind
    {
        assert_eq!(tool_name, "工具");
        assert_eq!(tool_use_id.as_deref(), Some("标识"));
    }
}

#[test]
fn special_characters_in_path() {
    let e = event(AgentEventKind::FileChanged {
        path: "src/my file (2).rs".into(),
        summary: "path with spaces & parens".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, .. } = &rt.kind {
        assert_eq!(path, "src/my file (2).rs");
    }
}

#[test]
fn newlines_in_output_preview() {
    let e = event(AgentEventKind::CommandExecuted {
        command: "cat".into(),
        exit_code: Some(0),
        output_preview: Some("line1\nline2\nline3".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { output_preview, .. } = &rt.kind {
        assert!(output_preview.as_ref().unwrap().contains('\n'));
    }
}

#[test]
fn very_long_message_roundtrip() {
    let long_msg = "x".repeat(100_000);
    let e = event(AgentEventKind::AssistantMessage {
        text: long_msg.clone(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text.len(), 100_000);
    }
}

// =========================================================================
// 20. Tool call / result correlation
// =========================================================================

#[test]
fn correlate_tool_calls_and_results_by_tool_use_id() {
    let events = vec![
        event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: Some("tu_2".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("data"),
            is_error: false,
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "write".into(),
            tool_use_id: Some("tu_2".into()),
            output: json!("ok"),
            is_error: false,
        }),
    ];

    // Collect call/result pairs
    let calls: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::ToolCall { tool_use_id, .. } = &e.kind {
                tool_use_id.clone()
            } else {
                None
            }
        })
        .collect();
    let results: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::ToolResult { tool_use_id, .. } = &e.kind {
                tool_use_id.clone()
            } else {
                None
            }
        })
        .collect();

    assert_eq!(calls, results);
}

#[test]
fn tool_call_without_tool_use_id() {
    let e = event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { tool_use_id, .. } = &rt.kind {
        assert!(tool_use_id.is_none());
    }
}

// =========================================================================
// 21. File change tracking
// =========================================================================

#[test]
fn track_multiple_file_changes() {
    let events = vec![
        event(AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "created".into(),
        }),
        event(AgentEventKind::FileChanged {
            path: "b.rs".into(),
            summary: "modified".into(),
        }),
        event(AgentEventKind::FileChanged {
            path: "c.rs".into(),
            summary: "deleted".into(),
        }),
    ];
    let paths: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::FileChanged { path, .. } = &e.kind {
                Some(path.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs"]);
}

#[test]
fn file_change_dedup_by_path() {
    let events = vec![
        event(AgentEventKind::FileChanged {
            path: "main.rs".into(),
            summary: "first edit".into(),
        }),
        event(AgentEventKind::FileChanged {
            path: "main.rs".into(),
            summary: "second edit".into(),
        }),
        event(AgentEventKind::FileChanged {
            path: "lib.rs".into(),
            summary: "new".into(),
        }),
    ];
    let mut paths: Vec<String> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::FileChanged { path, .. } = &e.kind {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect();
    paths.sort();
    paths.dedup();
    assert_eq!(paths, vec!["lib.rs", "main.rs"]);
}

// =========================================================================
// 22. Serialization: canonical JSON value equality
// =========================================================================

#[test]
fn serialize_deserialize_value_equality_all_variants() {
    for e in all_variant_events() {
        let v1 = serde_json::to_value(&e).unwrap();
        let json = serde_json::to_string(&e).unwrap();
        let v2: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v1, v2, "value mismatch for {}", type_name_of(&e.kind));
    }
}

#[test]
fn all_variants_contain_ts_and_type_in_json() {
    for e in all_variant_events() {
        let v = serde_json::to_value(&e).unwrap();
        assert!(
            v.get("ts").is_some(),
            "missing ts for {}",
            type_name_of(&e.kind)
        );
        assert!(
            v.get("type").is_some(),
            "missing type for {}",
            type_name_of(&e.kind)
        );
    }
}

// =========================================================================
// 23. Batch processing patterns
// =========================================================================

#[test]
fn count_events_by_type() {
    let events = vec![
        event(AgentEventKind::AssistantDelta { text: "a".into() }),
        event(AgentEventKind::AssistantDelta { text: "b".into() }),
        event(AgentEventKind::AssistantDelta { text: "c".into() }),
        event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::Warning {
            message: "w".into(),
        }),
    ];
    let delta_count = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
        .count();
    assert_eq!(delta_count, 3);
}

#[test]
fn partition_events_success_and_failure() {
    let events = vec![
        event(AgentEventKind::ToolResult {
            tool_name: "a".into(),
            tool_use_id: None,
            output: json!("ok"),
            is_error: false,
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "b".into(),
            tool_use_id: None,
            output: json!("err"),
            is_error: true,
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "c".into(),
            tool_use_id: None,
            output: json!("ok"),
            is_error: false,
        }),
    ];
    let (ok, err): (Vec<_>, Vec<_>) = events.iter().partition(|e| {
        if let AgentEventKind::ToolResult { is_error, .. } = &e.kind {
            !is_error
        } else {
            true
        }
    });
    assert_eq!(ok.len(), 2);
    assert_eq!(err.len(), 1);
}

#[test]
fn extract_all_tool_names() {
    let events = vec![
        event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
    ];
    let mut names: Vec<String> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::ToolCall { tool_name, .. } = &e.kind {
                Some(tool_name.clone())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names, vec!["read", "write"]);
}

// =========================================================================
// 24. Additional ErrorCode serde tests through AgentEvent
// =========================================================================

#[test]
fn error_code_all_categories_roundtrip() {
    let codes = vec![
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::PolicyDenied,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::DialectUnknown,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ];
    for code in codes {
        let e = event(AgentEventKind::Error {
            message: format!("test {}", code.as_str()),
            error_code: Some(code),
        });
        let rt = roundtrip(&e);
        if let AgentEventKind::Error {
            error_code: Some(rt_code),
            ..
        } = &rt.kind
        {
            assert_eq!(*rt_code, code, "roundtrip mismatch for {:?}", code);
        } else {
            panic!("error_code lost for {:?}", code);
        }
    }
}

#[test]
fn error_code_as_str_matches_serde() {
    let code = ErrorCode::BackendRateLimited;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, format!("\"{}\"", code.as_str()));
}

#[test]
fn error_code_deser_from_snake_case_string() {
    let code: ErrorCode = serde_json::from_str("\"protocol_version_mismatch\"").unwrap();
    assert_eq!(code, ErrorCode::ProtocolVersionMismatch);
}

// =========================================================================
// 25. Ext field BTreeMap ordering
// =========================================================================

#[test]
fn ext_btreemap_keys_sorted_in_json() {
    let mut ext = BTreeMap::new();
    ext.insert("zebra".into(), json!(1));
    ext.insert("alpha".into(), json!(2));
    ext.insert("mid".into(), json!(3));
    let e = event_with_ext(
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        ext,
    );
    let json = serde_json::to_string(&e).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let mid_pos = json.find("mid").unwrap();
    let zebra_pos = json.find("zebra").unwrap();
    assert!(alpha_pos < mid_pos);
    assert!(mid_pos < zebra_pos);
}
