// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for `AgentEvent` and `AgentEventKind`.

use std::collections::BTreeMap;

use abp_core::aggregate::{EventAggregator, RunAnalytics};
use abp_core::ext::AgentEventExt;
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: now(),
        kind,
        ext: None,
    }
}

fn event_at(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn roundtrip(ev: &AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(ev).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

// =========================================================================
// 1. Construction – one test per variant
// =========================================================================

#[test]
fn construct_run_started() {
    let ev = event(AgentEventKind::RunStarted {
        message: "begin".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn construct_run_completed() {
    let ev = event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn construct_assistant_delta() {
    let ev = event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn construct_assistant_message() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn construct_tool_call() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "foo.rs"}),
    });
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn construct_tool_result() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    });
    assert!(matches!(ev.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn construct_file_changed() {
    let ev = event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn construct_command_executed() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn construct_warning() {
    let ev = event(AgentEventKind::Warning {
        message: "deprecated API".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn construct_error() {
    let ev = event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

// =========================================================================
// 2. Serde roundtrip – every variant survives JSON encode/decode
// =========================================================================

#[test]
fn roundtrip_run_started() {
    let ev = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::RunStarted { message } if message == "go"));
}

#[test]
fn roundtrip_run_completed() {
    let ev = event(AgentEventKind::RunCompleted {
        message: "fin".into(),
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::RunCompleted { message } if message == "fin"));
}

#[test]
fn roundtrip_assistant_delta() {
    let ev = event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::AssistantDelta { text } if text == "chunk"));
}

#[test]
fn roundtrip_assistant_message() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::AssistantMessage { text } if text == "full msg"));
}

#[test]
fn roundtrip_tool_call() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "grep".into(),
        tool_use_id: Some("tc-99".into()),
        parent_tool_use_id: Some("tc-1".into()),
        input: json!({"q": "TODO"}),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        parent_tool_use_id,
        input,
    } = &rt.kind
    {
        assert_eq!(tool_name, "grep");
        assert_eq!(tool_use_id.as_deref(), Some("tc-99"));
        assert_eq!(parent_tool_use_id.as_deref(), Some("tc-1"));
        assert_eq!(input, &json!({"q": "TODO"}));
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn roundtrip_tool_result() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("ok"),
        is_error: true,
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolResult {
        tool_name,
        tool_use_id,
        output,
        is_error,
    } = &rt.kind
    {
        assert_eq!(tool_name, "bash");
        assert!(tool_use_id.is_none());
        assert_eq!(output, &json!("ok"));
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn roundtrip_file_changed() {
    let ev = event(AgentEventKind::FileChanged {
        path: "a/b.txt".into(),
        summary: "created".into(),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert_eq!(path, "a/b.txt");
        assert_eq!(summary, "created");
    } else {
        panic!("expected FileChanged");
    }
}

#[test]
fn roundtrip_command_executed() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(1),
        output_preview: None,
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &rt.kind
    {
        assert_eq!(command, "ls");
        assert_eq!(*exit_code, Some(1));
        assert!(output_preview.is_none());
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn roundtrip_warning() {
    let ev = event(AgentEventKind::Warning {
        message: "careful".into(),
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::Warning { message } if message == "careful"));
}

#[test]
fn roundtrip_error() {
    let ev = event(AgentEventKind::Error {
        message: "fatal".into(),
        error_code: None,
    });
    let rt = roundtrip(&ev);
    assert!(matches!(rt.kind, AgentEventKind::Error { message, .. } if message == "fatal"));
}

// =========================================================================
// 3. Discriminator tag – `"type"` field in serialized JSON
// =========================================================================

#[test]
fn tag_field_is_type() {
    let ev = event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert!(v.get("type").is_some(), "must have a 'type' field");
}

#[test]
fn tag_run_started_value() {
    let ev = event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "run_started");
}

#[test]
fn tag_run_completed_value() {
    let ev = event(AgentEventKind::RunCompleted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "run_completed");
}

#[test]
fn tag_assistant_delta_value() {
    let ev = event(AgentEventKind::AssistantDelta { text: "x".into() });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "assistant_delta");
}

#[test]
fn tag_assistant_message_value() {
    let ev = event(AgentEventKind::AssistantMessage { text: "x".into() });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "assistant_message");
}

#[test]
fn tag_tool_call_value() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "f".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "tool_call");
}

#[test]
fn tag_tool_result_value() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "f".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn tag_file_changed_value() {
    let ev = event(AgentEventKind::FileChanged {
        path: "x".into(),
        summary: "y".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "file_changed");
}

#[test]
fn tag_command_executed_value() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "command_executed");
}

#[test]
fn tag_warning_value() {
    let ev = event(AgentEventKind::Warning {
        message: "w".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "warning");
}

#[test]
fn tag_error_value() {
    let ev = event(AgentEventKind::Error {
        message: "e".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "error");
}

// =========================================================================
// 4. Timestamp handling
// =========================================================================

#[test]
fn timestamp_preserved_after_roundtrip() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 15, 12, 30, 45).unwrap();
    let ev = event_at(
        AgentEventKind::RunStarted {
            message: "ts test".into(),
        },
        ts,
    );
    let rt = roundtrip(&ev);
    assert_eq!(rt.ts, ts);
}

#[test]
fn timestamp_serialized_as_rfc3339() {
    let ev = event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    let ts_str = v["ts"].as_str().unwrap();
    // RFC 3339 ends with +00:00 or Z
    assert!(
        ts_str.ends_with("+00:00") || ts_str.ends_with('Z'),
        "timestamp should be UTC: {ts_str}"
    );
}

#[test]
fn timestamp_ordering_maintained() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
    let ev1 = event_at(
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        t1,
    );
    let ev2 = event_at(
        AgentEventKind::RunCompleted {
            message: "b".into(),
        },
        t2,
    );
    assert!(ev1.ts < ev2.ts);
}

#[test]
fn timestamp_subsecond_precision() {
    let ts = Utc::now();
    let ev = event_at(
        AgentEventKind::RunStarted {
            message: "precise".into(),
        },
        ts,
    );
    let rt = roundtrip(&ev);
    // chrono roundtrips via RFC 3339 preserve at least millisecond precision
    let diff = (rt.ts - ts).num_milliseconds().unsigned_abs();
    assert!(diff < 2, "subsecond precision lost: diff={diff}ms");
}

// =========================================================================
// 5. Event ordering and sequencing
// =========================================================================

#[test]
fn events_maintain_insertion_order_in_vec() {
    let kinds = vec![
        "run_started",
        "assistant_delta",
        "tool_call",
        "run_completed",
    ];
    let events: Vec<AgentEvent> = vec![
        event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        event(AgentEventKind::AssistantDelta { text: "hi".into() }),
        event(AgentEventKind::ToolCall {
            tool_name: "f".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    for (ev, expected) in events.iter().zip(&kinds) {
        let v: Value = serde_json::to_value(&ev.kind).unwrap();
        assert_eq!(v["type"].as_str().unwrap(), *expected);
    }
}

#[test]
fn stream_preserves_chronological_order() {
    let base = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let events: Vec<AgentEvent> = (0..5)
        .map(|i| {
            event_at(
                AgentEventKind::AssistantDelta {
                    text: format!("t{i}"),
                },
                base + chrono::Duration::seconds(i),
            )
        })
        .collect();
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[test]
fn stream_len_consistent_with_filter() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        event(AgentEventKind::RunCompleted {
            message: "b".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let filtered = stream.filter(&EventFilter::include_kinds(&["warning"]));
    assert_eq!(filtered.len(), 1);
    assert_eq!(stream.len(), 3);
}

// =========================================================================
// 6. Content types in events
// =========================================================================

#[test]
fn text_delta_carries_fragment() {
    let ev = event(AgentEventKind::AssistantDelta {
        text: "partial token".into(),
    });
    if let AgentEventKind::AssistantDelta { text } = &ev.kind {
        assert_eq!(text, "partial token");
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn tool_call_json_input_complex() {
    let input = json!({
        "files": ["a.rs", "b.rs"],
        "recursive": true,
        "depth": 3
    });
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "multi_edit".into(),
        tool_use_id: Some("x".into()),
        parent_tool_use_id: None,
        input: input.clone(),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolCall {
        input: rt_input, ..
    } = &rt.kind
    {
        assert_eq!(rt_input, &input);
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn tool_result_error_flag() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!({"stderr": "not found"}),
        is_error: true,
    });
    if let AgentEventKind::ToolResult { is_error, .. } = &ev.kind {
        assert!(is_error);
    }
}

#[test]
fn tool_result_success_flag() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("ok"),
        is_error: false,
    });
    if let AgentEventKind::ToolResult { is_error, .. } = &ev.kind {
        assert!(!is_error);
    }
}

#[test]
fn error_event_with_error_code() {
    let ev = event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn error_event_without_error_code() {
    let ev = event(AgentEventKind::Error {
        message: "unknown".into(),
        error_code: None,
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert!(
        v.get("error_code").is_none(),
        "None error_code should be skipped"
    );
}

#[test]
fn command_executed_with_all_fields() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(0),
        output_preview: Some("Finished".into()),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &rt.kind
    {
        assert_eq!(command, "cargo build");
        assert_eq!(*exit_code, Some(0));
        assert_eq!(output_preview.as_deref(), Some("Finished"));
    }
}

#[test]
fn command_executed_with_optional_fields_none() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "rm -rf /".into(),
        exit_code: None,
        output_preview: None,
    });
    let rt = roundtrip(&ev);
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

// =========================================================================
// 7. ext/metadata fields
// =========================================================================

#[test]
fn ext_none_omitted_in_json() {
    let ev = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert!(v.get("ext").is_none(), "ext=None should be omitted");
}

#[test]
fn ext_some_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    ext.insert("vendor".into(), json!("openai"));
    let ev = AgentEvent {
        ts: now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext.clone()),
    };
    let rt = roundtrip(&ev);
    assert_eq!(rt.ext, Some(ext));
}

#[test]
fn ext_empty_btreemap_roundtrips() {
    let ev = AgentEvent {
        ts: now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: Some(BTreeMap::new()),
    };
    let rt = roundtrip(&ev);
    assert_eq!(rt.ext, Some(BTreeMap::new()));
}

#[test]
fn ext_preserves_nested_json() {
    let mut ext = BTreeMap::new();
    ext.insert("deep".into(), json!({"a": {"b": {"c": [1, 2, 3]}}}));
    let ev = AgentEvent {
        ts: now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: Some(ext.clone()),
    };
    let rt = roundtrip(&ev);
    assert_eq!(rt.ext.unwrap()["deep"]["a"]["b"]["c"], json!([1, 2, 3]));
}

// =========================================================================
// 8. BTreeMap ordering in event metadata
// =========================================================================

#[test]
fn btreemap_ext_keys_sorted_in_json() {
    let mut ext = BTreeMap::new();
    ext.insert("zebra".into(), json!(1));
    ext.insert("apple".into(), json!(2));
    ext.insert("mango".into(), json!(3));
    let ev = AgentEvent {
        ts: now(),
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: Some(ext),
    };
    let json_str = serde_json::to_string(&ev).unwrap();
    let apple_pos = json_str.find("\"apple\"").unwrap();
    let mango_pos = json_str.find("\"mango\"").unwrap();
    let zebra_pos = json_str.find("\"zebra\"").unwrap();
    assert!(apple_pos < mango_pos);
    assert!(mango_pos < zebra_pos);
}

#[test]
fn btreemap_deterministic_serialization() {
    let mut ext = BTreeMap::new();
    ext.insert("z".into(), json!(1));
    ext.insert("a".into(), json!(2));
    let ev = AgentEvent {
        ts: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
        kind: AgentEventKind::RunStarted {
            message: "fixed".into(),
        },
        ext: Some(ext.clone()),
    };
    let s1 = serde_json::to_string(&ev).unwrap();
    let s2 = serde_json::to_string(&ev).unwrap();
    assert_eq!(s1, s2, "BTreeMap serialization must be deterministic");
}

// =========================================================================
// 9. Schema conformance
// =========================================================================

#[test]
fn agent_event_has_ts_field() {
    let ev = event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert!(v.get("ts").is_some());
}

#[test]
fn agent_event_kind_flattened() {
    // Because kind uses #[serde(flatten)], the kind fields appear at the
    // top level, not nested under "kind".
    let ev = event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert!(v.get("kind").is_none(), "kind should be flattened");
    assert!(
        v.get("type").is_some(),
        "type tag from kind should be at top level"
    );
    assert!(
        v.get("message").is_some(),
        "variant fields should be at top level"
    );
}

#[test]
fn tool_call_null_optional_fields_omitted_or_null() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "f".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    let v: Value = serde_json::to_value(&ev).unwrap();
    // tool_use_id is Option – should serialize as null
    assert!(v.get("tool_use_id").is_some());
    assert!(v["tool_use_id"].is_null());
}

#[test]
fn deserialization_from_raw_json_run_started() {
    let raw = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "run_started",
        "message": "hello"
    });
    let ev: AgentEvent = serde_json::from_value(raw).unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { message } if message == "hello"));
}

#[test]
fn deserialization_from_raw_json_tool_call() {
    let raw = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "tool_call",
        "tool_name": "edit",
        "tool_use_id": "t1",
        "parent_tool_use_id": null,
        "input": {"file": "x.rs"}
    });
    let ev: AgentEvent = serde_json::from_value(raw).unwrap();
    if let AgentEventKind::ToolCall { tool_name, .. } = &ev.kind {
        assert_eq!(tool_name, "edit");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn unknown_type_tag_rejected() {
    let raw = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "nonexistent_event",
        "message": "boom"
    });
    let result = serde_json::from_value::<AgentEvent>(raw);
    assert!(result.is_err());
}

// =========================================================================
// 10. Edge cases
// =========================================================================

#[test]
fn empty_string_message() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: String::new(),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert!(text.is_empty());
    }
}

#[test]
fn very_large_text_content() {
    let big = "x".repeat(100_000);
    let ev = event(AgentEventKind::AssistantMessage { text: big.clone() });
    let rt = roundtrip(&ev);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text.len(), 100_000);
        assert_eq!(text, &big);
    }
}

#[test]
fn unicode_in_text() {
    let text = "日本語テスト 🦀 émojis ñ";
    let ev = event(AgentEventKind::AssistantMessage { text: text.into() });
    let rt = roundtrip(&ev);
    if let AgentEventKind::AssistantMessage { text: t } = &rt.kind {
        assert_eq!(t, text);
    }
}

#[test]
fn unicode_in_tool_name() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "outil_français".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolCall { tool_name, .. } = &rt.kind {
        assert_eq!(tool_name, "outil_français");
    }
}

#[test]
fn special_chars_in_message() {
    let msg = r#"line1\nline2\ttab "quotes" <html>&amp;"#;
    let ev = event(AgentEventKind::Warning {
        message: msg.into(),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert_eq!(message, msg);
    }
}

#[test]
fn newlines_in_assistant_text() {
    let text = "line1\nline2\nline3";
    let ev = event(AgentEventKind::AssistantMessage { text: text.into() });
    let rt = roundtrip(&ev);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "line1\nline2\nline3");
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn null_json_in_tool_input() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "noop".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: Value::Null,
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert!(input.is_null());
    }
}

#[test]
fn array_json_in_tool_output() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "list".into(),
        tool_use_id: None,
        output: json!([1, 2, 3]),
        is_error: false,
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolResult { output, .. } = &rt.kind {
        assert_eq!(output, &json!([1, 2, 3]));
    }
}

#[test]
fn negative_exit_code() {
    let ev = event(AgentEventKind::CommandExecuted {
        command: "kill".into(),
        exit_code: Some(-1),
        output_preview: None,
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &rt.kind {
        assert_eq!(*exit_code, Some(-1));
    }
}

#[test]
fn empty_path_in_file_changed() {
    let ev = event(AgentEventKind::FileChanged {
        path: String::new(),
        summary: String::new(),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert!(path.is_empty());
        assert!(summary.is_empty());
    }
}

// =========================================================================
// 11. Event stream aggregation
// =========================================================================

#[test]
fn aggregator_counts_events() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    agg.add(&event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    }));
    agg.add(&event(AgentEventKind::RunCompleted {
        message: "done".into(),
    }));
    assert_eq!(agg.event_count(), 3);
}

#[test]
fn aggregator_counts_by_kind() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::AssistantDelta { text: "a".into() }));
    agg.add(&event(AgentEventKind::AssistantDelta { text: "b".into() }));
    agg.add(&event(AgentEventKind::ToolCall {
        tool_name: "f".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    }));
    let counts = agg.count_by_kind();
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["tool_call"], 1);
}

#[test]
fn aggregator_has_errors_true() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    }));
    assert!(agg.has_errors());
}

#[test]
fn aggregator_has_errors_false() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::Warning {
        message: "meh".into(),
    }));
    assert!(!agg.has_errors());
}

#[test]
fn aggregator_text_length() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    }));
    agg.add(&event(AgentEventKind::AssistantDelta {
        text: "world".into(),
    }));
    assert_eq!(agg.text_length(), 10);
}

#[test]
fn aggregator_tool_calls_list() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    }));
    agg.add(&event(AgentEventKind::ToolCall {
        tool_name: "write".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    }));
    assert_eq!(agg.tool_calls(), vec!["read", "write"]);
}

#[test]
fn aggregator_unique_tool_count() {
    let mut agg = EventAggregator::new();
    for name in &["read", "write", "read", "bash", "write"] {
        agg.add(&event(AgentEventKind::ToolCall {
            tool_name: (*name).into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }));
    }
    assert_eq!(agg.unique_tool_count(), 3);
}

#[test]
fn aggregator_error_messages() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::Error {
        message: "err1".into(),
        error_code: None,
    }));
    agg.add(&event(AgentEventKind::Error {
        message: "err2".into(),
        error_code: None,
    }));
    assert_eq!(agg.error_messages(), vec!["err1", "err2"]);
}

#[test]
fn aggregator_summary_snapshot() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    agg.add(&event(AgentEventKind::AssistantMessage {
        text: "abc".into(),
    }));
    let summary = agg.summary();
    assert_eq!(summary.total_events, 2);
    assert_eq!(summary.total_text_chars, 3);
    assert_eq!(summary.tool_calls, 0);
    assert_eq!(summary.errors, 0);
}

#[test]
fn run_analytics_tool_usage_ratio() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "f".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "f".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        }),
        event(AgentEventKind::RunCompleted {
            message: "b".into(),
        }),
    ];
    let analytics = RunAnalytics::from_events(&events);
    let ratio = analytics.tool_usage_ratio();
    assert!((ratio - 0.25).abs() < f64::EPSILON);
}

#[test]
fn run_analytics_empty_events() {
    let analytics = RunAnalytics::from_events(&[]);
    assert_eq!(analytics.tool_usage_ratio(), 0.0);
    assert_eq!(analytics.average_text_per_event(), 0.0);
    assert!(analytics.is_successful());
}

// =========================================================================
// 12. Event filtering by kind
// =========================================================================

#[test]
fn filter_include_single_kind() {
    let f = EventFilter::include_kinds(&["error"]);
    let err = event(AgentEventKind::Error {
        message: "bad".into(),
        error_code: None,
    });
    let warn = event(AgentEventKind::Warning {
        message: "meh".into(),
    });
    assert!(f.matches(&err));
    assert!(!f.matches(&warn));
}

#[test]
fn filter_include_multiple_kinds() {
    let f = EventFilter::include_kinds(&["error", "warning"]);
    let err = event(AgentEventKind::Error {
        message: "bad".into(),
        error_code: None,
    });
    let warn = event(AgentEventKind::Warning {
        message: "meh".into(),
    });
    let msg = event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert!(f.matches(&err));
    assert!(f.matches(&warn));
    assert!(!f.matches(&msg));
}

#[test]
fn filter_exclude_removes_matching() {
    let f = EventFilter::exclude_kinds(&["assistant_delta"]);
    let delta = event(AgentEventKind::AssistantDelta { text: "x".into() });
    let msg = event(AgentEventKind::AssistantMessage { text: "y".into() });
    assert!(!f.matches(&delta));
    assert!(f.matches(&msg));
}

#[test]
fn filter_empty_include_passes_nothing() {
    let f = EventFilter::include_kinds(&[]);
    let ev = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    assert!(!f.matches(&ev));
}

#[test]
fn filter_empty_exclude_passes_everything() {
    let f = EventFilter::exclude_kinds(&[]);
    let ev = event(AgentEventKind::RunStarted {
        message: "a".into(),
    });
    assert!(f.matches(&ev));
}

#[test]
fn event_stream_by_kind() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        event(AgentEventKind::AssistantDelta { text: "b".into() }),
        event(AgentEventKind::AssistantDelta { text: "c".into() }),
        event(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let deltas = stream.by_kind("assistant_delta");
    assert_eq!(deltas.len(), 2);
}

#[test]
fn event_stream_count_by_kind() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "f".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "g".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        event(AgentEventKind::RunCompleted {
            message: "b".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let counts = stream.count_by_kind();
    assert_eq!(counts["tool_call"], 2);
    assert_eq!(counts["run_started"], 1);
}

#[test]
fn event_stream_first_and_last_of_kind() {
    let events = vec![
        event(AgentEventKind::AssistantDelta {
            text: "first".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "middle".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "last".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let first = stream.first_of_kind("assistant_delta").unwrap();
    let last = stream.last_of_kind("assistant_delta").unwrap();
    assert_eq!(first.text_content(), Some("first"));
    assert_eq!(last.text_content(), Some("last"));
}

// =========================================================================
// Additional: AgentEventExt trait tests
// =========================================================================

#[test]
fn ext_is_tool_call_true() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "f".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    assert!(ev.is_tool_call());
}

#[test]
fn ext_is_tool_call_false_for_result() {
    let ev = event(AgentEventKind::ToolResult {
        tool_name: "f".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    });
    assert!(!ev.is_tool_call());
}

#[test]
fn ext_is_terminal_for_run_completed() {
    let ev = event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(ev.is_terminal());
}

#[test]
fn ext_is_terminal_false_for_run_started() {
    let ev = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!ev.is_terminal());
}

#[test]
fn ext_text_content_delta() {
    let ev = event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert_eq!(ev.text_content(), Some("tok"));
}

#[test]
fn ext_text_content_message() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "full".into(),
    });
    assert_eq!(ev.text_content(), Some("full"));
}

#[test]
fn ext_text_content_none_for_other() {
    let ev = event(AgentEventKind::Warning {
        message: "w".into(),
    });
    assert!(ev.text_content().is_none());
}

// =========================================================================
// Additional: clone and debug
// =========================================================================

#[test]
fn agent_event_clone() {
    let ev = event(AgentEventKind::AssistantMessage {
        text: "clone me".into(),
    });
    let cloned = ev.clone();
    assert_eq!(
        serde_json::to_string(&ev).unwrap(),
        serde_json::to_string(&cloned).unwrap()
    );
}

#[test]
fn agent_event_debug_output() {
    let ev = event(AgentEventKind::RunStarted {
        message: "debug".into(),
    });
    let dbg = format!("{ev:?}");
    assert!(dbg.contains("RunStarted"));
    assert!(dbg.contains("debug"));
}

// =========================================================================
// Additional: case-insensitive filter matching
// =========================================================================

#[test]
fn filter_case_insensitive() {
    let f = EventFilter::include_kinds(&["RUN_STARTED"]);
    let ev = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(f.matches(&ev));
}

#[test]
fn stream_by_kind_case_insensitive() {
    let events = vec![event(AgentEventKind::Warning {
        message: "w".into(),
    })];
    let stream = EventStream::new(events);
    let result = stream.by_kind("WARNING");
    assert_eq!(result.len(), 1);
}

// =========================================================================
// Additional: tool call with parent tool use id (nesting)
// =========================================================================

#[test]
fn tool_call_nested_parent_id_roundtrip() {
    let ev = event(AgentEventKind::ToolCall {
        tool_name: "sub_tool".into(),
        tool_use_id: Some("child-1".into()),
        parent_tool_use_id: Some("parent-0".into()),
        input: json!({"nested": true}),
    });
    let rt = roundtrip(&ev);
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &rt.kind
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("parent-0"));
    }
}

// =========================================================================
// Additional: full run sequence roundtrip
// =========================================================================

#[test]
fn full_run_sequence_roundtrip() {
    let events = vec![
        event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "I will ".into(),
        }),
        event(AgentEventKind::AssistantDelta {
            text: "help you".into(),
        }),
        event(AgentEventKind::AssistantMessage {
            text: "I will help you".into(),
        }),
        event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        }),
        event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc1".into()),
            output: json!("fn main() {}"),
            is_error: false,
        }),
        event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added logging".into(),
        }),
        event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Finished".into()),
        }),
        event(AgentEventKind::Warning {
            message: "unused variable".into(),
        }),
        event(AgentEventKind::RunCompleted {
            message: "all done".into(),
        }),
    ];
    let json_str = serde_json::to_string(&events).unwrap();
    let deserialized: Vec<AgentEvent> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(events.len(), deserialized.len());

    // Verify each type tag roundtripped correctly
    let expected_types = [
        "run_started",
        "assistant_delta",
        "assistant_delta",
        "assistant_message",
        "tool_call",
        "tool_result",
        "file_changed",
        "command_executed",
        "warning",
        "run_completed",
    ];
    for (ev, expected_type) in deserialized.iter().zip(&expected_types) {
        let v: Value = serde_json::to_value(&ev.kind).unwrap();
        assert_eq!(v["type"].as_str().unwrap(), *expected_type);
    }
}
