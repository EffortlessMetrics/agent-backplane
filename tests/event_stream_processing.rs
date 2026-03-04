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
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Comprehensive tests for AgentEvent stream processing.
//!
//! Covers variant creation, serde roundtrips, ext fields, stream ordering,
//! filtering, aggregation, JSONL serialization, channel behavior, and more.
#![allow(clippy::useless_vec)]

use std::collections::{BTreeMap, HashMap};

use abp_core::{AgentEvent, AgentEventKind};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;
use tokio::sync::mpsc;

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

fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn make_event_at(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn roundtrip(event: &AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(event).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn roundtrip_value(event: &AgentEvent) -> serde_json::Value {
    serde_json::to_value(event).expect("to_value")
}

/// Build a typical event stream: started → deltas → tool_call → tool_result → completed.
fn sample_stream() -> Vec<AgentEvent> {
    let base = fixed_ts();
    vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            base,
        ),
        make_event_at(
            AgentEventKind::AssistantDelta { text: "Hel".into() },
            base + chrono::Duration::milliseconds(10),
        ),
        make_event_at(
            AgentEventKind::AssistantDelta { text: "lo".into() },
            base + chrono::Duration::milliseconds(20),
        ),
        make_event_at(
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "a.txt"}),
            },
            base + chrono::Duration::milliseconds(30),
        ),
        make_event_at(
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: json!("contents"),
                is_error: false,
            },
            base + chrono::Duration::milliseconds(40),
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            base + chrono::Duration::milliseconds(50),
        ),
    ]
}

// ===========================================================================
// 1. Variant creation
// ===========================================================================

#[test]
fn create_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "begin".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn create_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "fin".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn create_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn create_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn create_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("u1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    });
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn create_tool_result() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("u1".into()),
        output: json!("ok"),
        is_error: false,
    });
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn create_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added fn".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn create_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo build".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn create_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "caution".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn create_error() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(matches!(e.kind, AgentEventKind::Error { .. }));
}

#[test]
fn create_error_with_code() {
    let e = make_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    } else {
        panic!("wrong variant");
    }
}

// ===========================================================================
// 2. Timestamp handling
// ===========================================================================

#[test]
fn timestamp_preserved_after_roundtrip() {
    let ts = fixed_ts();
    let e = make_event_at(
        AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ts,
    );
    let rt = roundtrip(&e);
    assert_eq!(rt.ts, ts);
}

#[test]
fn timestamp_ordering() {
    let stream = sample_stream();
    for w in stream.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[test]
fn timestamp_subsecond_precision() {
    let ts = Utc::now();
    let e = make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts);
    let rt = roundtrip(&e);
    assert_eq!(rt.ts.timestamp_nanos_opt(), ts.timestamp_nanos_opt());
}

#[test]
fn timestamp_epoch() {
    let ts = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
    let e = make_event_at(AgentEventKind::RunStarted { message: "".into() }, ts);
    let rt = roundtrip(&e);
    assert_eq!(rt.ts, ts);
}

#[test]
fn timestamp_far_future() {
    let ts = Utc.with_ymd_and_hms(2099, 12, 31, 23, 59, 59).unwrap();
    let e = make_event_at(AgentEventKind::RunStarted { message: "".into() }, ts);
    let rt = roundtrip(&e);
    assert_eq!(rt.ts, ts);
}

// ===========================================================================
// 3. Serde roundtrip for each variant
// ===========================================================================

#[test]
fn serde_roundtrip_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "hello".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert_eq!(message, "hello");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_roundtrip_run_completed() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunCompleted { message } = &rt.kind {
        assert_eq!(message, "done");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "chunk");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_assistant_message() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "full".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "full");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_tool_call() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "grep".into(),
        tool_use_id: Some("id1".into()),
        parent_tool_use_id: Some("pid".into()),
        input: json!({"q": "pattern"}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        parent_tool_use_id,
        input,
    } = &rt.kind
    {
        assert_eq!(tool_name, "grep");
        assert_eq!(tool_use_id.as_deref(), Some("id1"));
        assert_eq!(parent_tool_use_id.as_deref(), Some("pid"));
        assert_eq!(input, &json!({"q": "pattern"}));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_tool_call_minimal() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "noop".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
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
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_tool_result() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("u1".into()),
        output: json!({"data": [1,2,3]}),
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult {
        tool_name,
        output,
        is_error,
        ..
    } = &rt.kind
    {
        assert_eq!(tool_name, "read");
        assert_eq!(output, &json!({"data": [1,2,3]}));
        assert!(!is_error);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_tool_result_error() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "write".into(),
        tool_use_id: None,
        output: json!("permission denied"),
        is_error: true,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { is_error, .. } = &rt.kind {
        assert!(is_error);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "lib.rs".into(),
        summary: "refactor".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert_eq!(path, "lib.rs");
        assert_eq!(summary, "refactor");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "echo hi".into(),
        exit_code: Some(0),
        output_preview: Some("hi\n".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &rt.kind
    {
        assert_eq!(command, "echo hi");
        assert_eq!(*exit_code, Some(0));
        assert_eq!(output_preview.as_deref(), Some("hi\n"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_command_executed_minimal() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
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
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_warning() {
    let e = make_event(AgentEventKind::Warning {
        message: "slow".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert_eq!(message, "slow");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_error_no_code() {
    let e = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &rt.kind
    {
        assert_eq!(message, "fail");
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_roundtrip_error_with_code() {
    let e = make_event(AgentEventKind::Error {
        message: "denied".into(),
        error_code: Some(abp_error::ErrorCode::PolicyDenied),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::PolicyDenied));
    } else {
        panic!("wrong variant");
    }
}

// ===========================================================================
// 4. Ext field handling
// ===========================================================================

#[test]
fn ext_none_omitted_in_json() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let v = roundtrip_value(&e);
    assert!(v.get("ext").is_none());
}

#[test]
fn ext_roundtrip_single_key() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext.clone()),
    };
    let rt = roundtrip(&e);
    assert_eq!(
        rt.ext.as_ref().unwrap()["raw_message"],
        json!({"role": "assistant"})
    );
}

#[test]
fn ext_roundtrip_multiple_keys() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!("raw"));
    ext.insert("vendor".into(), json!("openai"));
    ext.insert("seq".into(), json!(42));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    let rt_ext = rt.ext.unwrap();
    assert_eq!(rt_ext.len(), 3);
    assert_eq!(rt_ext["seq"], json!(42));
}

#[test]
fn ext_with_nested_json() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"deeply": {"nested": {"value": true}}}),
    );
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert_eq!(
        rt.ext.unwrap()["raw_message"]["deeply"]["nested"]["value"],
        json!(true)
    );
}

#[test]
fn ext_with_array_value() {
    let mut ext = BTreeMap::new();
    ext.insert("items".into(), json!([1, 2, 3]));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert_eq!(rt.ext.unwrap()["items"], json!([1, 2, 3]));
}

#[test]
fn ext_empty_map_roundtrips() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(BTreeMap::new()),
    };
    let rt = roundtrip(&e);
    // Empty map may serialize as {} or be preserved
    assert!(rt.ext.is_some());
}

#[test]
fn ext_null_value_in_map() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::Value::Null);
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert!(rt.ext.unwrap()["key"].is_null());
}

// ===========================================================================
// 5. Event stream ordering
// ===========================================================================

#[test]
fn stream_timestamps_monotonically_increase() {
    let events = sample_stream();
    for pair in events.windows(2) {
        assert!(
            pair[0].ts <= pair[1].ts,
            "timestamps out of order: {:?} > {:?}",
            pair[0].ts,
            pair[1].ts
        );
    }
}

#[test]
fn stream_starts_with_run_started() {
    let events = sample_stream();
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn stream_ends_with_run_completed() {
    let events = sample_stream();
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_tool_result_follows_tool_call() {
    let events = sample_stream();
    for (i, e) in events.iter().enumerate() {
        if let AgentEventKind::ToolResult { tool_use_id, .. } = &e.kind {
            // Find matching tool_call before this index
            let found = events[..i].iter().any(|prev| {
                if let AgentEventKind::ToolCall {
                    tool_use_id: prev_id,
                    ..
                } = &prev.kind
                {
                    prev_id == tool_use_id
                } else {
                    false
                }
            });
            assert!(found, "tool_result without preceding tool_call");
        }
    }
}

#[test]
fn stream_no_events_after_run_completed() {
    let events = sample_stream();
    let completed_idx = events
        .iter()
        .position(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    if let Some(idx) = completed_idx {
        assert_eq!(idx, events.len() - 1, "events exist after RunCompleted");
    }
}

// ===========================================================================
// 6. Event filtering by kind
// ===========================================================================

#[test]
fn filter_only_deltas() {
    let events = sample_stream();
    let deltas: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
        .collect();
    assert_eq!(deltas.len(), 2);
}

#[test]
fn filter_only_tool_calls() {
    let events = sample_stream();
    let calls: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .collect();
    assert_eq!(calls.len(), 1);
}

#[test]
fn filter_only_tool_results() {
    let events = sample_stream();
    let results: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .collect();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_terminal_events() {
    let events = sample_stream();
    let terminal: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }))
        .collect();
    assert_eq!(terminal.len(), 1);
}

#[test]
fn filter_no_warnings_in_clean_stream() {
    let events = sample_stream();
    let warnings: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Warning { .. }))
        .collect();
    assert!(warnings.is_empty());
}

#[test]
fn filter_no_errors_in_clean_stream() {
    let events = sample_stream();
    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .collect();
    assert!(errors.is_empty());
}

#[test]
fn filter_text_bearing_events() {
    let events = sample_stream();
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

// ===========================================================================
// 7. Event aggregation (count by type)
// ===========================================================================

fn type_tag(kind: &AgentEventKind) -> &'static str {
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

#[test]
fn aggregate_counts_sample_stream() {
    let events = sample_stream();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for e in &events {
        *counts.entry(type_tag(&e.kind)).or_default() += 1;
    }
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["run_completed"], 1);
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["tool_call"], 1);
    assert_eq!(counts["tool_result"], 1);
    assert_eq!(counts.get("warning"), None);
}

#[test]
fn aggregate_counts_empty_stream() {
    let events: Vec<AgentEvent> = vec![];
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for e in &events {
        *counts.entry(type_tag(&e.kind)).or_default() += 1;
    }
    assert!(counts.is_empty());
}

#[test]
fn aggregate_counts_all_variants() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "".into() }),
        make_event(AgentEventKind::AssistantMessage { text: "".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "".into(),
            summary: "".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "".into(),
            exit_code: None,
            output_preview: None,
        }),
        make_event(AgentEventKind::Warning { message: "".into() }),
        make_event(AgentEventKind::Error {
            message: "".into(),
            error_code: None,
        }),
    ];
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for e in &events {
        *counts.entry(type_tag(&e.kind)).or_default() += 1;
    }
    assert_eq!(counts.len(), 10);
}

// ===========================================================================
// 8. JSONL serialization
// ===========================================================================

#[test]
fn jsonl_each_event_is_single_line() {
    let events = sample_stream();
    for e in &events {
        let line = serde_json::to_string(e).unwrap();
        assert!(!line.contains('\n'), "JSONL line must not contain newlines");
    }
}

#[test]
fn jsonl_format_full_stream() {
    let events = sample_stream();
    let jsonl: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), events.len());
}

#[test]
fn jsonl_parse_back_each_line() {
    let events = sample_stream();
    let jsonl: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for line in jsonl.lines() {
        let _: AgentEvent = serde_json::from_str(line).expect("parse JSONL line");
    }
}

#[test]
fn jsonl_trailing_newline_tolerant() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let line = serde_json::to_string(&e).unwrap();
    let with_trailing = format!("{}\n", line);
    let parsed: AgentEvent = serde_json::from_str(with_trailing.trim()).unwrap();
    assert!(matches!(parsed.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn jsonl_type_discriminator_present() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "x".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let v = roundtrip_value(&e);
    assert_eq!(v["type"], "tool_call");
}

#[test]
fn jsonl_type_discriminator_for_all_variants() {
    let variants: Vec<(&str, AgentEventKind)> = vec![
        (
            "run_started",
            AgentEventKind::RunStarted { message: "".into() },
        ),
        (
            "run_completed",
            AgentEventKind::RunCompleted { message: "".into() },
        ),
        (
            "assistant_delta",
            AgentEventKind::AssistantDelta { text: "".into() },
        ),
        (
            "assistant_message",
            AgentEventKind::AssistantMessage { text: "".into() },
        ),
        (
            "tool_call",
            AgentEventKind::ToolCall {
                tool_name: "".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!(null),
            },
        ),
        (
            "tool_result",
            AgentEventKind::ToolResult {
                tool_name: "".into(),
                tool_use_id: None,
                output: json!(null),
                is_error: false,
            },
        ),
        (
            "file_changed",
            AgentEventKind::FileChanged {
                path: "".into(),
                summary: "".into(),
            },
        ),
        (
            "command_executed",
            AgentEventKind::CommandExecuted {
                command: "".into(),
                exit_code: None,
                output_preview: None,
            },
        ),
        ("warning", AgentEventKind::Warning { message: "".into() }),
        (
            "error",
            AgentEventKind::Error {
                message: "".into(),
                error_code: None,
            },
        ),
    ];
    for (expected_tag, kind) in variants {
        let e = make_event(kind);
        let v = roundtrip_value(&e);
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_tag,
            "mismatch for {}",
            expected_tag
        );
    }
}

// ===========================================================================
// 9. Empty / minimal events
// ===========================================================================

#[test]
fn empty_message_run_started() {
    let e = make_event(AgentEventKind::RunStarted {
        message: String::new(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert!(message.is_empty());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn empty_text_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: String::new(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert!(text.is_empty());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn empty_tool_name() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: String::new(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { tool_name, .. } = &rt.kind {
        assert!(tool_name.is_empty());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn minimal_file_changed() {
    let e = make_event(AgentEventKind::FileChanged {
        path: String::new(),
        summary: String::new(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert!(path.is_empty());
        assert!(summary.is_empty());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn minimal_command_executed() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: String::new(),
        exit_code: None,
        output_preview: None,
    });
    roundtrip(&e); // Just ensure no panic
}

// ===========================================================================
// 10. Large payloads
// ===========================================================================

#[test]
fn large_assistant_delta() {
    let big = "x".repeat(100_000);
    let e = make_event(AgentEventKind::AssistantDelta { text: big.clone() });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text.len(), 100_000);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn large_tool_input() {
    let arr: Vec<i32> = (0..10_000).collect();
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "big".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(arr),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert_eq!(input.as_array().unwrap().len(), 10_000);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn large_tool_output() {
    let data: HashMap<String, String> = (0..1000)
        .map(|i| (format!("key_{i}"), format!("value_{i}")))
        .collect();
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "big".into(),
        tool_use_id: None,
        output: serde_json::to_value(data).unwrap(),
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { output, .. } = &rt.kind {
        assert_eq!(output.as_object().unwrap().len(), 1000);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn large_ext_field() {
    let mut ext = BTreeMap::new();
    for i in 0..500 {
        ext.insert(format!("k{i}"), json!(i));
    }
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert_eq!(rt.ext.unwrap().len(), 500);
}

#[test]
fn large_command_output_preview() {
    let big_preview = "line\n".repeat(10_000);
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cat big.log".into(),
        exit_code: Some(0),
        output_preview: Some(big_preview.clone()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { output_preview, .. } = &rt.kind {
        assert_eq!(output_preview.as_ref().unwrap().len(), big_preview.len());
    } else {
        panic!("wrong variant");
    }
}

// ===========================================================================
// 11. Unicode in event fields
// ===========================================================================

#[test]
fn unicode_assistant_delta() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "こんにちは世界 🌍".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "こんにちは世界 🌍");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_tool_name() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "工具_名前".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { tool_name, .. } = &rt.kind {
        assert_eq!(tool_name, "工具_名前");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_file_path() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/données/résumé.txt".into(),
        summary: "modifié".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert_eq!(path, "src/données/résumé.txt");
        assert_eq!(summary, "modifié");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_emoji_in_message() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "🚀 Starting agent run ✅".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert!(message.contains('🚀'));
        assert!(message.contains('✅'));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_in_tool_input_json() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({"query": "Ñoño café"}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert_eq!(input["query"], "Ñoño café");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_warning_message() {
    let e = make_event(AgentEventKind::Warning {
        message: "⚠️ Achtung: Überlauf".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert!(message.contains("Überlauf"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unicode_in_ext_keys_and_values() {
    let mut ext = BTreeMap::new();
    ext.insert("clé".into(), json!("valeur_ñ"));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert_eq!(rt.ext.unwrap()["clé"], json!("valeur_ñ"));
}

// ===========================================================================
// 12. Event channel behavior
// ===========================================================================

#[tokio::test]
async fn channel_buffered_send_recv() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);
    let e = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    tx.send(e).await.unwrap();
    let received = rx.recv().await.unwrap();
    assert!(matches!(received.kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn channel_stream_multiple_events() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(64);
    let events = sample_stream();
    let count = events.len();
    for e in events {
        tx.send(e).await.unwrap();
    }
    drop(tx);
    let mut received = Vec::new();
    while let Some(e) = rx.recv().await {
        received.push(e);
    }
    assert_eq!(received.len(), count);
}

#[tokio::test]
async fn channel_preserves_order() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(64);
    let events = sample_stream();
    for e in events.clone() {
        tx.send(e).await.unwrap();
    }
    drop(tx);
    let mut idx = 0;
    while let Some(e) = rx.recv().await {
        assert_eq!(e.ts, events[idx].ts);
        idx += 1;
    }
}

#[tokio::test]
async fn channel_closed_after_drop() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(4);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    let _ = rx.recv().await; // consume the one event
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn channel_unbuffered_blocks_appropriately() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(1);
    let e1 = make_event(AgentEventKind::AssistantDelta { text: "a".into() });
    tx.send(e1).await.unwrap();
    // Channel is now full (capacity=1). Receive to unblock.
    let received = rx.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

#[tokio::test]
async fn channel_try_send_when_full() {
    let (tx, _rx) = mpsc::channel::<AgentEvent>(1);
    let e1 = make_event(AgentEventKind::RunStarted { message: "".into() });
    let e2 = make_event(AgentEventKind::RunCompleted { message: "".into() });
    tx.send(e1).await.unwrap();
    // Channel full; try_send should fail
    assert!(tx.try_send(e2).is_err());
}

#[tokio::test]
async fn channel_recv_returns_none_when_all_senders_dropped() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(8);
    drop(tx);
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn channel_multiple_producers() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(32);
    let tx2 = tx.clone();

    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx2.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();
    drop(tx);
    drop(tx2);

    let mut count = 0;
    while rx.recv().await.is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
}

// ===========================================================================
// 13. Event deduplication
// ===========================================================================

#[test]
fn dedup_by_timestamp_and_kind_tag() {
    let ts = fixed_ts();
    let events = vec![
        make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts),
        make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts),
        make_event_at(AgentEventKind::AssistantDelta { text: "b".into() }, ts),
    ];
    // Dedup by serialized JSON
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<_> = events
        .into_iter()
        .filter(|e| seen.insert(serde_json::to_string(e).unwrap()))
        .collect();
    assert_eq!(unique.len(), 2); // "a" deduped, "b" kept
}

#[test]
fn dedup_different_timestamps_not_deduped() {
    let ts1 = fixed_ts();
    let ts2 = ts1 + chrono::Duration::milliseconds(1);
    let events = vec![
        make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts1),
        make_event_at(AgentEventKind::AssistantDelta { text: "a".into() }, ts2),
    ];
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<_> = events
        .into_iter()
        .filter(|e| seen.insert(serde_json::to_string(e).unwrap()))
        .collect();
    assert_eq!(unique.len(), 2);
}

#[test]
fn dedup_different_kinds_not_deduped() {
    let ts = fixed_ts();
    let events = vec![
        make_event_at(
            AgentEventKind::RunStarted {
                message: "x".into(),
            },
            ts,
        ),
        make_event_at(
            AgentEventKind::RunCompleted {
                message: "x".into(),
            },
            ts,
        ),
    ];
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<_> = events
        .into_iter()
        .filter(|e| seen.insert(serde_json::to_string(e).unwrap()))
        .collect();
    assert_eq!(unique.len(), 2);
}

#[test]
fn dedup_ext_difference_prevents_dedup() {
    let ts = fixed_ts();
    let mut ext = BTreeMap::new();
    ext.insert("k".into(), json!(1));
    let e1 = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: None,
    };
    let e2 = AgentEvent {
        ts,
        kind: AgentEventKind::RunStarted {
            message: "x".into(),
        },
        ext: Some(ext),
    };
    let s1 = serde_json::to_string(&e1).unwrap();
    let s2 = serde_json::to_string(&e2).unwrap();
    assert_ne!(s1, s2);
}

// ===========================================================================
// 14. Event sequence validation
// ===========================================================================

fn validate_sequence(events: &[AgentEvent]) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }
    // First event should be RunStarted
    if !matches!(events[0].kind, AgentEventKind::RunStarted { .. }) {
        return Err("first event is not RunStarted".into());
    }
    // Track tool calls awaiting results
    let mut pending_tool_ids: Vec<Option<String>> = Vec::new();
    for e in events {
        match &e.kind {
            AgentEventKind::ToolCall { tool_use_id, .. } => {
                pending_tool_ids.push(tool_use_id.clone());
            }
            AgentEventKind::ToolResult { tool_use_id, .. } => {
                if let Some(pos) = pending_tool_ids.iter().position(|id| id == tool_use_id) {
                    pending_tool_ids.remove(pos);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[test]
fn sequence_valid_sample_stream() {
    assert!(validate_sequence(&sample_stream()).is_ok());
}

#[test]
fn sequence_empty_is_valid() {
    assert!(validate_sequence(&[]).is_ok());
}

#[test]
fn sequence_missing_run_started_fails() {
    let events = vec![make_event(AgentEventKind::AssistantDelta {
        text: "x".into(),
    })];
    assert!(validate_sequence(&events).is_err());
}

#[test]
fn sequence_run_started_only() {
    let events = vec![make_event(AgentEventKind::RunStarted {
        message: "".into(),
    })];
    assert!(validate_sequence(&events).is_ok());
}

#[test]
fn sequence_multiple_tool_calls_and_results() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "a".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!(null),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "b".into(),
            tool_use_id: Some("t2".into()),
            parent_tool_use_id: None,
            input: json!(null),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "a".into(),
            tool_use_id: Some("t1".into()),
            output: json!("ok"),
            is_error: false,
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "b".into(),
            tool_use_id: Some("t2".into()),
            output: json!("ok"),
            is_error: false,
        }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
    ];
    assert!(validate_sequence(&events).is_ok());
}

#[test]
fn sequence_nested_tool_calls() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "outer".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!(null),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "inner".into(),
            tool_use_id: Some("t2".into()),
            parent_tool_use_id: Some("t1".into()),
            input: json!(null),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "inner".into(),
            tool_use_id: Some("t2".into()),
            output: json!("inner_result"),
            is_error: false,
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "outer".into(),
            tool_use_id: Some("t1".into()),
            output: json!("outer_result"),
            is_error: false,
        }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
    ];
    assert!(validate_sequence(&events).is_ok());
}

#[test]
fn sequence_interleaved_deltas_and_tools() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::AssistantDelta {
            text: "thinking...".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: json!("data"),
            is_error: false,
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "based on that...".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    assert!(validate_sequence(&events).is_ok());
}

// ===========================================================================
// 15. RunCompleted as terminal event
// ===========================================================================

#[test]
fn run_completed_is_terminal() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn run_completed_preserves_message() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "success with 3 edits".into(),
    });
    if let AgentEventKind::RunCompleted { message } = &e.kind {
        assert_eq!(message, "success with 3 edits");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_completed_serde_roundtrip_message() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "All tasks completed".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::RunCompleted { message } = &rt.kind {
        assert_eq!(message, "All tasks completed");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_completed_last_in_stream() {
    let stream = sample_stream();
    let last = stream.last().unwrap();
    assert!(matches!(last.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn run_completed_terminates_channel_consumer() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);
    for e in sample_stream() {
        tx.send(e).await.unwrap();
    }
    drop(tx);

    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        let is_terminal = matches!(e.kind, AgentEventKind::RunCompleted { .. });
        events.push(e);
        if is_terminal {
            break;
        }
    }
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// ===========================================================================
// Additional: Clone, Debug, JSON shape, error codes exhaustive
// ===========================================================================

#[test]
fn agent_event_is_clone() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let c = e.clone();
    assert_eq!(
        serde_json::to_string(&e).unwrap(),
        serde_json::to_string(&c).unwrap()
    );
}

#[test]
fn agent_event_is_debug() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("RunStarted"));
}

#[test]
fn json_does_not_contain_kind_key() {
    // `kind` is flattened, so the JSON should not have a "kind" key
    let e = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let v = roundtrip_value(&e);
    assert!(
        v.get("kind").is_none(),
        "flattened kind should not appear as 'kind'"
    );
}

#[test]
fn json_contains_ts_key() {
    let e = make_event(AgentEventKind::RunStarted { message: "".into() });
    let v = roundtrip_value(&e);
    assert!(v.get("ts").is_some());
}

#[test]
fn error_code_backend_not_found_roundtrips() {
    let e = make_event(AgentEventKind::Error {
        message: "not found".into(),
        error_code: Some(abp_error::ErrorCode::BackendNotFound),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendNotFound));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_code_backend_crashed_roundtrips() {
    let e = make_event(AgentEventKind::Error {
        message: "crash".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendCrashed));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_code_protocol_invalid_envelope_roundtrips() {
    let e = make_event(AgentEventKind::Error {
        message: "bad envelope".into(),
        error_code: Some(abp_error::ErrorCode::ProtocolInvalidEnvelope),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(
            *error_code,
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_code_internal_roundtrips() {
    let e = make_event(AgentEventKind::Error {
        message: "internal".into(),
        error_code: Some(abp_error::ErrorCode::Internal),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::Internal));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn error_code_serializes_screaming_snake() {
    let e = make_event(AgentEventKind::Error {
        message: "".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let v = roundtrip_value(&e);
    assert_eq!(v["error_code"], "backend_timeout");
}

// ===========================================================================
// Additional: Deserialization from raw JSON
// ===========================================================================

#[test]
fn deserialize_run_started_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"run_started","message":"go"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn deserialize_assistant_delta_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"assistant_delta","text":"hi"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &e.kind {
        assert_eq!(text, "hi");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn deserialize_tool_call_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"tool_call","tool_name":"bash","tool_use_id":"u1","parent_tool_use_id":null,"input":{"cmd":"ls"}}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn deserialize_tool_result_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"tool_result","tool_name":"bash","tool_use_id":"u1","output":"ok","is_error":false}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn deserialize_file_changed_from_json() {
    let json =
        r#"{"ts":"2025-01-15T12:00:00Z","type":"file_changed","path":"a.rs","summary":"edit"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn deserialize_command_executed_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"command_executed","command":"ls","exit_code":0,"output_preview":"files"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn deserialize_warning_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"warning","message":"warn"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn deserialize_error_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"error","message":"fail"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::Error { .. }));
}

#[test]
fn deserialize_error_with_code_from_json() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"error","message":"timeout","error_code":"backend_timeout"}"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn deserialize_rejects_unknown_type() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","type":"unknown_variant","data":"x"}"#;
    let result = serde_json::from_str::<AgentEvent>(json);
    assert!(result.is_err());
}

#[test]
fn deserialize_rejects_missing_type() {
    let json = r#"{"ts":"2025-01-15T12:00:00Z","message":"x"}"#;
    let result = serde_json::from_str::<AgentEvent>(json);
    assert!(result.is_err());
}

// ===========================================================================
// Additional: Deterministic serialization (BTreeMap ext)
// ===========================================================================

#[test]
fn ext_btreemap_deterministic_order() {
    let mut ext = BTreeMap::new();
    ext.insert("z_last".into(), json!(1));
    ext.insert("a_first".into(), json!(2));
    ext.insert("m_middle".into(), json!(3));
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted { message: "".into() },
        ext: Some(ext),
    };
    let json1 = serde_json::to_string(&e).unwrap();
    let json2 = serde_json::to_string(&e).unwrap();
    assert_eq!(
        json1, json2,
        "BTreeMap ext must serialize deterministically"
    );
    // Keys should appear in alphabetical order
    let a_pos = json1.find("a_first").unwrap();
    let m_pos = json1.find("m_middle").unwrap();
    let z_pos = json1.find("z_last").unwrap();
    assert!(a_pos < m_pos && m_pos < z_pos);
}

// ===========================================================================
// Additional: Special characters & edge cases
// ===========================================================================

#[test]
fn newlines_in_text_fields() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "line1\nline2\nline3".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "line1\nline2\nline3");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn tab_characters_in_output() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cat".into(),
        exit_code: Some(0),
        output_preview: Some("col1\tcol2\tcol3".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { output_preview, .. } = &rt.kind {
        assert!(output_preview.as_ref().unwrap().contains('\t'));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn backslash_in_path() {
    let e = make_event(AgentEventKind::FileChanged {
        path: r"src\main.rs".into(),
        summary: "windows path".into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::FileChanged { path, .. } = &rt.kind {
        assert!(path.contains('\\'));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn quotes_in_message() {
    let e = make_event(AgentEventKind::Warning {
        message: r#"value is "null""#.into(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert!(message.contains('"'));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn null_bytes_in_output() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cat binary".into(),
        exit_code: Some(0),
        output_preview: Some("before\0after".into()),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { output_preview, .. } = &rt.kind {
        assert!(output_preview.as_ref().unwrap().contains('\0'));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn tool_input_complex_nested_json() {
    let input = json!({
        "files": [
            {"path": "a.rs", "content": "fn main() {}"},
            {"path": "b.rs", "content": "mod tests;"}
        ],
        "options": {
            "recursive": true,
            "depth": 5,
            "filters": ["*.rs", "*.toml"]
        }
    });
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "multi_edit".into(),
        tool_use_id: Some("m1".into()),
        parent_tool_use_id: None,
        input: input.clone(),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall {
        input: rt_input, ..
    } = &rt.kind
    {
        assert_eq!(*rt_input, input);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn negative_exit_code() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "segfault".into(),
        exit_code: Some(-11),
        output_preview: None,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &rt.kind {
        assert_eq!(*exit_code, Some(-11));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn tool_result_with_null_output() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "void".into(),
        tool_use_id: None,
        output: serde_json::Value::Null,
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { output, .. } = &rt.kind {
        assert!(output.is_null());
    } else {
        panic!("wrong variant");
    }
}

// ===========================================================================
// Additional: Stream concatenation and splitting
// ===========================================================================

#[test]
fn concatenate_delta_texts() {
    let events = sample_stream();
    let full_text: String = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::AssistantDelta { text } = &e.kind {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(full_text, "Hello");
}

#[test]
fn count_tool_round_trips() {
    let events = sample_stream();
    let calls = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .count();
    let results = events
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }))
        .count();
    assert_eq!(calls, results, "every tool call should have a result");
}

#[test]
fn stream_duration_calculation() {
    let events = sample_stream();
    let first_ts = events.first().unwrap().ts;
    let last_ts = events.last().unwrap().ts;
    let duration = last_ts - first_ts;
    assert_eq!(duration.num_milliseconds(), 50);
}

#[tokio::test]
async fn channel_stream_with_warning_and_error() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "disk low".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "crash".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        }),
    ];
    for e in &events {
        tx.send(e.clone()).await.unwrap();
    }
    drop(tx);

    let mut warnings = 0;
    let mut errors = 0;
    while let Some(e) = rx.recv().await {
        match &e.kind {
            AgentEventKind::Warning { .. } => warnings += 1,
            AgentEventKind::Error { .. } => errors += 1,
            _ => {}
        }
    }
    assert_eq!(warnings, 1);
    assert_eq!(errors, 1);
}

#[test]
fn partition_events_by_category() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "x".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        make_event(AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
    ];
    let (lifecycle, rest): (Vec<_>, Vec<_>) = events.iter().partition(|e| {
        matches!(
            e.kind,
            AgentEventKind::RunStarted { .. } | AgentEventKind::RunCompleted { .. }
        )
    });
    assert_eq!(lifecycle.len(), 2);
    assert_eq!(rest.len(), 4);
}

#[test]
fn collect_file_changes_from_stream() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "created".into(),
        }),
        make_event(AgentEventKind::FileChanged {
            path: "b.rs".into(),
            summary: "modified".into(),
        }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
    ];
    let paths: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::FileChanged { path, .. } = &e.kind {
                Some(path.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(paths, vec!["a.rs", "b.rs"]);
}

#[test]
fn collect_commands_from_stream() {
    let events = vec![
        make_event(AgentEventKind::RunStarted { message: "".into() }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: None,
        }),
        make_event(AgentEventKind::RunCompleted { message: "".into() }),
    ];
    let cmds: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::CommandExecuted { command, .. } = &e.kind {
                Some(command.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(cmds, vec!["cargo build", "cargo test"]);
}
