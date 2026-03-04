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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the event streaming system — AgentEvent types,
//! streaming behaviour, envelope wrapping, and channel-based delivery.

use std::collections::BTreeMap;

use abp_core::{AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, WorkOrderBuilder};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{Duration, Utc};
use serde_json::json;

// =========================================================================
// Helpers
// =========================================================================

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[allow(dead_code)]
fn make_event_at(kind: AgentEventKind, offset_ms: i64) -> AgentEvent {
    AgentEvent {
        ts: Utc::now() + Duration::milliseconds(offset_ms),
        kind,
        ext: None,
    }
}

// =========================================================================
// 1. AgentEvent types (~15 tests)
// =========================================================================

#[test]
fn construct_run_started() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn construct_run_completed() {
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn construct_assistant_delta() {
    let ev = make_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn construct_assistant_message() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn construct_tool_call() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    });
    assert!(matches!(ev.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn construct_tool_result() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        output: json!("file contents here"),
        is_error: false,
    });
    assert!(matches!(ev.kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn construct_file_changed() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn construct_command_executed() {
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    });
    assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
}

#[test]
fn construct_warning() {
    let ev = make_event(AgentEventKind::Warning {
        message: "something odd".into(),
    });
    assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn construct_error() {
    let ev = make_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[test]
fn agent_event_has_timestamp() {
    let before = Utc::now();
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let after = Utc::now();
    assert!(ev.ts >= before && ev.ts <= after);
}

#[test]
fn agent_event_has_kind() {
    let ev = make_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    if let AgentEventKind::Warning { message } = &ev.kind {
        assert_eq!(message, "w");
    } else {
        panic!("expected Warning");
    }
}

#[test]
fn agent_event_ext_defaults_to_none() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "s".into(),
    });
    assert!(ev.ext.is_none());
}

#[test]
fn agent_event_ext_can_carry_data() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "test"}));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    assert!(ev.ext.is_some());
    assert!(ev.ext.unwrap().contains_key("raw_message"));
}

#[test]
fn agent_event_debug_impl() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "debug-test".into(),
    });
    let dbg = format!("{:?}", ev);
    assert!(dbg.contains("RunStarted"));
    assert!(dbg.contains("debug-test"));
}

// =========================================================================
// 2. Event kind coverage (~15 tests)
// =========================================================================

#[test]
fn serde_roundtrip_run_started() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::RunStarted { message } if message == "start"));
}

#[test]
fn serde_roundtrip_run_completed() {
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "fin".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::RunCompleted { message } if message == "fin"));
}

#[test]
fn serde_roundtrip_assistant_delta() {
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::AssistantDelta { text } if text == "chunk"));
}

#[test]
fn serde_roundtrip_assistant_message() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::AssistantMessage { text } if text == "full msg"));
}

#[test]
fn serde_roundtrip_tool_call() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu-42".into()),
        parent_tool_use_id: None,
        input: json!({"command": "ls"}),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name, input, ..
    } = &de.kind
    {
        assert_eq!(tool_name, "bash");
        assert_eq!(input["command"], "ls");
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn serde_roundtrip_tool_result() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu-42".into()),
        output: json!("ok"),
        is_error: false,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult {
        tool_name,
        is_error,
        ..
    } = &de.kind
    {
        assert_eq!(tool_name, "bash");
        assert!(!is_error);
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn serde_roundtrip_tool_result_error() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!({"error": "command not found"}),
        is_error: true,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolResult { is_error, .. } = &de.kind {
        assert!(is_error);
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn serde_roundtrip_file_changed() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "a.txt".into(),
        summary: "created".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::FileChanged { path, .. } if path == "a.txt"));
}

#[test]
fn serde_roundtrip_command_executed() {
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "echo hi".into(),
        exit_code: Some(0),
        output_preview: Some("hi".into()),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted {
        command, exit_code, ..
    } = &de.kind
    {
        assert_eq!(command, "echo hi");
        assert_eq!(*exit_code, Some(0));
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn serde_roundtrip_warning() {
    let ev = make_event(AgentEventKind::Warning {
        message: "watch out".into(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.kind, AgentEventKind::Warning { message } if message == "watch out"));
}

#[test]
fn serde_roundtrip_error_with_code() {
    let ev = make_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &de.kind
    {
        assert_eq!(message, "fail");
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn serde_error_without_code_omits_field() {
    let ev = make_event(AgentEventKind::Error {
        message: "oops".into(),
        error_code: None,
    });
    let json = serde_json::to_string(&ev).unwrap();
    // error_code is skip_serializing_if = "Option::is_none"
    assert!(!json.contains("error_code"));
}

#[test]
fn tool_call_with_nested_parent_id() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "inner_tool".into(),
        tool_use_id: Some("child-1".into()),
        parent_tool_use_id: Some("parent-1".into()),
        input: json!({}),
    });
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &ev.kind
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
    }
}

#[test]
fn command_executed_no_exit_code() {
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "sleep 99".into(),
        exit_code: None,
        output_preview: None,
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &de.kind {
        assert!(exit_code.is_none());
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn event_kind_tag_field_is_type() {
    let ev = make_event(AgentEventKind::AssistantDelta { text: "t".into() });
    let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
    // AgentEventKind uses #[serde(tag = "type")]
    assert_eq!(v["type"], "assistant_delta");
}

// =========================================================================
// 3. Envelope wrapping (~15 tests)
// =========================================================================

#[test]
fn wrap_event_in_envelope() {
    let ev = make_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event: ev,
    };
    assert!(matches!(envelope, Envelope::Event { .. }));
}

#[test]
fn envelope_event_serde_roundtrip() {
    let ev = make_event(AgentEventKind::Warning {
        message: "warn".into(),
    });
    let envelope = Envelope::Event {
        ref_id: "r-1".into(),
        event: ev,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "r-1");
        assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
    } else {
        panic!("expected Event envelope");
    }
}

#[test]
fn envelope_discriminator_is_t() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "x".into(),
    });
    let envelope = Envelope::Event {
        ref_id: "r".into(),
        event: ev,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn hello_envelope_has_t_hello() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn fatal_envelope_has_t_fatal() {
    let fatal = Envelope::Fatal {
        ref_id: Some("r-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn envelope_ref_id_correlation() {
    let run_id = "run-abc-123";
    let ev = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
    let envelope = Envelope::Event {
        ref_id: run_id.into(),
        event: ev,
    };
    if let Envelope::Event { ref_id, .. } = &envelope {
        assert_eq!(ref_id, run_id);
    }
}

#[test]
fn multiple_events_share_ref_id() {
    let ref_id = "run-shared";
    let envs: Vec<Envelope> = (0..5)
        .map(|i| Envelope::Event {
            ref_id: ref_id.into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            }),
        })
        .collect();
    for env in &envs {
        if let Envelope::Event { ref_id: rid, .. } = env {
            assert_eq!(rid, ref_id);
        }
    }
}

#[test]
fn envelope_jsonl_ends_with_newline() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&envelope).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn envelope_decode_stream_multiple() {
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "e1".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e2".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let reader = std::io::BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn envelope_decode_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 1);
}

#[test]
fn envelope_fatal_with_error_code() {
    let fatal = Envelope::fatal_with_code(
        Some("r1".into()),
        "timed out",
        abp_error::ErrorCode::BackendTimeout,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::BackendTimeout)
    );
}

#[test]
fn envelope_event_does_not_have_error_code() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let envelope = Envelope::Event {
        ref_id: "r".into(),
        event: ev,
    };
    assert!(envelope.error_code().is_none());
}

#[test]
fn envelope_hello_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "t".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    if let Envelope::Hello {
        contract_version, ..
    } = &hello
    {
        assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn run_envelope_carries_work_order() {
    let wo = WorkOrderBuilder::new("test task").build();
    let task = wo.task.clone();
    let id = wo.id.to_string();
    let run = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    if let Envelope::Run {
        id: rid,
        work_order,
    } = &run
    {
        assert_eq!(rid, &id);
        assert_eq!(work_order.task, task);
    }
}

#[test]
fn run_envelope_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("roundtrip test").build();
    let run = Envelope::Run {
        id: "r-rt".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

// =========================================================================
// 4. Stream lifecycle (~15 tests)
// =========================================================================

#[test]
fn event_stream_new() {
    let stream = abp_core::stream::EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
}

#[test]
fn event_stream_with_events() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    assert_eq!(stream.len(), 2);
}

#[test]
fn stream_starts_with_run_started() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "begin".into(),
        }),
        make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "end".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let first = stream.first_of_kind("run_started");
    assert!(first.is_some());
}

#[test]
fn stream_ends_with_run_completed() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "end".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let last = stream.last_of_kind("run_completed");
    assert!(last.is_some());
}

#[test]
fn stream_error_mid_stream() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "partial".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "failed".into(),
            error_code: None,
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    assert_eq!(stream.len(), 3);
    assert!(stream.first_of_kind("error").is_some());
}

#[test]
fn stream_empty_handling() {
    let stream = abp_core::stream::EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert!(stream.first_of_kind("run_started").is_none());
    assert!(stream.last_of_kind("run_completed").is_none());
    assert!(stream.duration().is_none());
}

#[test]
fn stream_single_event() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "lone".into(),
    })];
    let stream = abp_core::stream::EventStream::new(events);
    assert_eq!(stream.len(), 1);
    assert!(stream.duration().is_none()); // < 2 events
}

#[test]
fn stream_by_kind_filters_correctly() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "t1".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "t2".into() }),
        make_event(AgentEventKind::RunCompleted {
            message: "e".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let deltas = stream.by_kind("assistant_delta");
    assert_eq!(deltas.len(), 2);
}

#[test]
fn stream_count_by_kind() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "t1".into() }),
        make_event(AgentEventKind::AssistantDelta { text: "t2".into() }),
        make_event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "e".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let counts = stream.count_by_kind();
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["warning"], 1);
}

#[test]
fn stream_filter_include() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let filter = abp_core::filter::EventFilter::include_kinds(&["warning", "error"]);
    let filtered = stream.filter(&filter);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn stream_filter_exclude() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "noisy".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "e".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let filter = abp_core::filter::EventFilter::exclude_kinds(&["assistant_delta"]);
    let filtered = stream.filter(&filter);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn stream_into_iterator() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "e".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let collected: Vec<_> = stream.into_iter().collect();
    assert_eq!(collected.len(), 2);
}

#[test]
fn stream_ref_iterator() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "e".into(),
        }),
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let count = (&stream).into_iter().count();
    assert_eq!(count, 2);
    // stream is not consumed
    assert_eq!(stream.len(), 2);
}

#[test]
fn stream_duration_with_multiple_events() {
    let base = Utc::now();
    let events = vec![
        AgentEvent {
            ts: base,
            kind: AgentEventKind::RunStarted {
                message: "s".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(100),
            kind: AgentEventKind::AssistantMessage { text: "m".into() },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(500),
            kind: AgentEventKind::RunCompleted {
                message: "e".into(),
            },
            ext: None,
        },
    ];
    let stream = abp_core::stream::EventStream::new(events);
    let dur = stream.duration().unwrap();
    assert!(dur.as_millis() >= 400);
}

// =========================================================================
// 5. Event validation (~10 tests)
// =========================================================================

#[test]
fn error_event_with_valid_error_code() {
    let ev = make_event(AgentEventKind::Error {
        message: "timed out".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    if let AgentEventKind::Error { error_code, .. } = &ev.kind {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
    }
}

#[test]
fn tool_call_has_valid_tool_name() {
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    if let AgentEventKind::ToolCall { tool_name, .. } = &ev.kind {
        assert!(!tool_name.is_empty());
    }
}

#[test]
fn tool_result_has_valid_tool_name() {
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "write_file".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    });
    if let AgentEventKind::ToolResult { tool_name, .. } = &ev.kind {
        assert!(!tool_name.is_empty());
    }
}

#[test]
fn assistant_delta_content_preserved() {
    let content = "Hello, world! 🌍";
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: content.into(),
    });
    if let AgentEventKind::AssistantDelta { text } = &ev.kind {
        assert_eq!(text, content);
    }
}

#[test]
fn file_changed_path_non_empty() {
    let ev = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "new file".into(),
    });
    if let AgentEventKind::FileChanged { path, .. } = &ev.kind {
        assert!(!path.is_empty());
    }
}

#[test]
fn error_event_message_non_empty() {
    let ev = make_event(AgentEventKind::Error {
        message: "something went wrong".into(),
        error_code: None,
    });
    if let AgentEventKind::Error { message, .. } = &ev.kind {
        assert!(!message.is_empty());
    }
}

#[test]
fn warning_event_message_non_empty() {
    let ev = make_event(AgentEventKind::Warning {
        message: "caution".into(),
    });
    if let AgentEventKind::Warning { message } = &ev.kind {
        assert!(!message.is_empty());
    }
}

#[test]
fn tool_call_input_is_valid_json() {
    let input = json!({"path": "/tmp/test.rs", "content": "fn main() {}"});
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "write_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: input.clone(),
    });
    if let AgentEventKind::ToolCall { input: inp, .. } = &ev.kind {
        assert_eq!(*inp, input);
    }
}

#[test]
fn event_timestamp_is_utc() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    // The ts field is DateTime<Utc> — validate it serializes with 'Z' or '+00:00'
    let json = serde_json::to_string(&ev).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let ts_str = v["ts"].as_str().unwrap();
    // chrono Utc serializes with Z suffix
    assert!(
        ts_str.ends_with('Z') || ts_str.contains("+00:00"),
        "timestamp should be UTC: {ts_str}"
    );
}

#[test]
fn event_ordering_by_timestamp() {
    let base = Utc::now();
    #[allow(clippy::useless_vec)]
    let mut events = vec![
        AgentEvent {
            ts: base + Duration::milliseconds(200),
            kind: AgentEventKind::RunCompleted {
                message: "end".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base,
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base + Duration::milliseconds(100),
            kind: AgentEventKind::AssistantMessage { text: "mid".into() },
            ext: None,
        },
    ];
    events.sort_by_key(|e| e.ts);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// =========================================================================
// 6. Channel-based streaming (~10 tests)
// =========================================================================

#[tokio::test]
async fn mpsc_channel_delivers_events() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(16);
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    tx.send(ev).await.unwrap();
    drop(tx);
    let received = rx.recv().await.unwrap();
    assert!(matches!(
        received.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn mpsc_channel_preserves_order() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(16);
    for i in 0..10 {
        let ev = make_event(AgentEventKind::AssistantDelta {
            text: format!("tok-{i}"),
        });
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let mut received = Vec::new();
    while let Some(ev) = rx.recv().await {
        received.push(ev);
    }
    assert_eq!(received.len(), 10);
    for (i, ev) in received.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("tok-{i}"));
        } else {
            panic!("expected AssistantDelta");
        }
    }
}

#[tokio::test]
async fn mpsc_channel_closing_signals_end() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(4);
    tx.send(make_event(AgentEventKind::RunStarted {
        message: "s".into(),
    }))
    .await
    .unwrap();
    drop(tx); // close the channel
    let _ = rx.recv().await; // consume the event
    let end = rx.recv().await;
    assert!(end.is_none(), "channel should signal end after drop");
}

#[tokio::test]
async fn mpsc_send_after_receiver_dropped_fails() {
    let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(4);
    drop(rx);
    let result = tx
        .send(make_event(AgentEventKind::RunStarted {
            message: "orphan".into(),
        }))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn mpsc_bounded_channel_respects_capacity() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(2);

    // Fill the channel
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "a".into(),
    }))
    .await
    .unwrap();
    tx.send(make_event(AgentEventKind::AssistantDelta {
        text: "b".into(),
    }))
    .await
    .unwrap();

    // Drain and verify
    let e1 = rx.recv().await.unwrap();
    let e2 = rx.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(e2.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn mock_backend_streams_events_via_channel() {
    use abp_backend_core::Backend;
    use abp_backend_mock::MockBackend;
    use uuid::Uuid;

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test streaming").build();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(32);

    let _receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    assert!(events.len() >= 2, "should have at least start + complete");

    // First event should be RunStarted
    assert!(
        matches!(
            events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ),
        "first event should be RunStarted"
    );

    // Last event should be RunCompleted
    assert!(
        matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
}

#[tokio::test]
async fn mock_backend_receipt_contains_trace() {
    use abp_backend_core::Backend;
    use abp_backend_mock::MockBackend;
    use uuid::Uuid;

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("trace test").build();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Drain the channel
    while rx.recv().await.is_some() {}

    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should contain events"
    );
    assert!(receipt.receipt_sha256.is_some(), "receipt should have hash");
}

#[tokio::test]
async fn channel_events_match_receipt_trace() {
    use abp_backend_core::Backend;
    use abp_backend_mock::MockBackend;
    use uuid::Uuid;

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("match test").build();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    let mut channel_events = Vec::new();
    while let Some(ev) = rx.recv().await {
        channel_events.push(ev);
    }

    // Channel events and receipt trace should have the same count
    assert_eq!(
        channel_events.len(),
        receipt.trace.len(),
        "channel events and trace should match in count"
    );
}

#[tokio::test]
async fn concurrent_event_collection() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);

    let producer = tokio::spawn(async move {
        for i in 0..20 {
            let kind = if i == 0 {
                AgentEventKind::RunStarted {
                    message: "go".into(),
                }
            } else if i == 19 {
                AgentEventKind::RunCompleted {
                    message: "done".into(),
                }
            } else {
                AgentEventKind::AssistantDelta {
                    text: format!("tok-{i}"),
                }
            };
            tx.send(make_event(kind)).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });

    producer.await.unwrap();
    let events = consumer.await.unwrap();
    assert_eq!(events.len(), 20);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[19].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// =========================================================================
// 7. StreamParser incremental parsing
// =========================================================================

#[test]
fn stream_parser_partial_line() {
    use abp_protocol::stream::StreamParser;

    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    })
    .unwrap();
    let bytes = line.as_bytes();
    let (first, second) = bytes.split_at(bytes.len() / 2);

    let r1 = parser.push(first);
    assert!(r1.is_empty(), "partial line should produce no results");

    let r2 = parser.push(second);
    assert_eq!(r2.len(), 1, "completing the line should produce one result");
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_at_once() {
    use abp_protocol::stream::StreamParser;

    let mut parser = StreamParser::new();
    let e1 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "a".into(),
        error_code: None,
    })
    .unwrap();
    let e2 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "b".into(),
        error_code: None,
    })
    .unwrap();
    let combined = format!("{e1}{e2}");
    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_finish_flushes_buffer() {
    use abp_protocol::stream::StreamParser;

    let mut parser = StreamParser::new();
    // Push a line without trailing newline
    let json = r#"{"t":"fatal","ref_id":null,"error":"no-newline"}"#;
    let r = parser.push(json.as_bytes());
    assert!(r.is_empty());

    let r = parser.finish();
    assert_eq!(r.len(), 1);
    assert!(r[0].is_ok());
}

#[test]
fn stream_parser_reset_clears_buffer() {
    use abp_protocol::stream::StreamParser;

    let mut parser = StreamParser::new();
    parser.push(b"partial data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_invalid_json_returns_error() {
    use abp_protocol::stream::StreamParser;

    let mut parser = StreamParser::new();
    let results = parser.push(b"not valid json\n");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// =========================================================================
// 8. Event serde edge cases
// =========================================================================

#[test]
fn event_with_ext_serde_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor_field": 42}));
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "with ext".into(),
        },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(de.ext.is_some());
    assert_eq!(de.ext.unwrap()["raw_message"]["vendor_field"], 42);
}

#[test]
fn event_without_ext_omits_field() {
    let ev = make_event(AgentEventKind::AssistantDelta { text: "t".into() });
    let json = serde_json::to_string(&ev).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        v.get("ext").is_none(),
        "ext:None should be omitted from JSON"
    );
}

#[test]
fn event_clone_is_equal() {
    let ev = make_event(AgentEventKind::AssistantMessage {
        text: "clone me".into(),
    });
    let cloned = ev.clone();
    assert_eq!(ev.ts, cloned.ts);
    let orig_json = serde_json::to_string(&ev).unwrap();
    let clone_json = serde_json::to_string(&cloned).unwrap();
    assert_eq!(orig_json, clone_json);
}

#[test]
fn all_event_kinds_produce_type_field() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
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
        let v = serde_json::to_value(&kind).unwrap();
        assert!(
            v.get("type").is_some(),
            "kind {kind:?} should have 'type' field"
        );
    }
}

#[test]
fn event_kind_rename_all_snake_case() {
    let expected: Vec<(&str, AgentEventKind)> = vec![
        (
            "run_started",
            AgentEventKind::RunStarted {
                message: String::new(),
            },
        ),
        (
            "run_completed",
            AgentEventKind::RunCompleted {
                message: String::new(),
            },
        ),
        (
            "assistant_delta",
            AgentEventKind::AssistantDelta {
                text: String::new(),
            },
        ),
        (
            "assistant_message",
            AgentEventKind::AssistantMessage {
                text: String::new(),
            },
        ),
        (
            "tool_call",
            AgentEventKind::ToolCall {
                tool_name: String::new(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!(null),
            },
        ),
        (
            "tool_result",
            AgentEventKind::ToolResult {
                tool_name: String::new(),
                tool_use_id: None,
                output: json!(null),
                is_error: false,
            },
        ),
        (
            "file_changed",
            AgentEventKind::FileChanged {
                path: String::new(),
                summary: String::new(),
            },
        ),
        (
            "command_executed",
            AgentEventKind::CommandExecuted {
                command: String::new(),
                exit_code: None,
                output_preview: None,
            },
        ),
        (
            "warning",
            AgentEventKind::Warning {
                message: String::new(),
            },
        ),
        (
            "error",
            AgentEventKind::Error {
                message: String::new(),
                error_code: None,
            },
        ),
    ];

    for (name, kind) in expected {
        let v = serde_json::to_value(&kind).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            name,
            "variant should serialize as {name}"
        );
    }
}

#[test]
fn envelope_event_discriminator_vs_kind_discriminator() {
    // Envelope uses tag="t", AgentEventKind uses tag="type"
    let ev = make_event(AgentEventKind::Warning {
        message: "test".into(),
    });
    let envelope = Envelope::Event {
        ref_id: "r".into(),
        event: ev,
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    // Top-level discriminator is "t"
    assert_eq!(v["t"], "event");
    // Nested event kind discriminator is "type"
    assert_eq!(v["event"]["type"], "warning");
}

#[test]
fn large_tool_call_input_roundtrips() {
    let large_input: serde_json::Value = (0..100)
        .map(|i| (format!("key_{i}"), json!(format!("value_{i}"))))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "complex_tool".into(),
        tool_use_id: Some("tu-large".into()),
        parent_tool_use_id: None,
        input: large_input.clone(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall { input, .. } = &de.kind {
        assert_eq!(*input, large_input);
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn unicode_content_roundtrips() {
    let text = "Hello 🌍 世界 مرحبا Привет";
    let ev = make_event(AgentEventKind::AssistantMessage { text: text.into() });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text: t } = &de.kind {
        assert_eq!(t, text);
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn empty_string_fields_roundtrip() {
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: String::new(),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantDelta { text } = &de.kind {
        assert!(text.is_empty());
    }
}

#[tokio::test]
async fn many_events_through_channel_preserves_all() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);
    let count = 100;

    let producer = tokio::spawn(async move {
        for i in 0..count {
            tx.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            }))
            .await
            .unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });

    producer.await.unwrap();
    let events = consumer.await.unwrap();
    assert_eq!(events.len(), count);
}

#[test]
fn event_from_json_string_literal() {
    let json = r#"{"ts":"2024-01-01T00:00:00Z","type":"assistant_message","text":"from json"}"#;
    let ev: AgentEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::AssistantMessage { ref text } if text == "from json"
    ));
}

#[test]
fn envelope_event_from_json_string() {
    let json = r#"{"t":"event","ref_id":"r-1","event":{"ts":"2024-01-01T00:00:00Z","type":"run_started","message":"go"}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Event { ref_id, event } = env {
        assert_eq!(ref_id, "r-1");
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event envelope");
    }
}
