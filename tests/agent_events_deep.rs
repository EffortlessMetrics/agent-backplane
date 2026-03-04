// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for AgentEvent construction, streaming, and type coverage.
//!
//! Covers: event type construction and field access, stream buffer/metrics/tee
//! behavior, and event-to-protocol envelope mapping.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::aggregate::{EventAggregator, RunAnalytics};
use abp_core::ext::AgentEventExt;
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use abp_stream::{StreamBuffer, StreamMetrics, StreamTee};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now() -> chrono::DateTime<Utc> {
    Utc::now()
}

fn ev(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: now(),
        kind,
        ext: None,
    }
}

fn ev_at(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn roundtrip(e: &AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(e).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn delta(text: &str) -> AgentEvent {
    ev(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn make_ref_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// =========================================================================
// a) Event type coverage (15 tests)
// =========================================================================

#[test]
fn text_delta_event_construction_and_fields() {
    let e = ev(AgentEventKind::AssistantDelta {
        text: "partial".into(),
    });
    if let AgentEventKind::AssistantDelta { text } = &e.kind {
        assert_eq!(text, "partial");
    } else {
        panic!("expected AssistantDelta");
    }
}

#[test]
fn tool_call_start_event_with_tool_name_and_id() {
    let e = ev(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-42".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    });
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        input,
        ..
    } = &e.kind
    {
        assert_eq!(tool_name, "read_file");
        assert_eq!(tool_use_id.as_deref(), Some("tu-42"));
        assert_eq!(input["path"], "src/main.rs");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn tool_call_delta_with_partial_arguments() {
    // Tool calls carry the full input; simulate partial by sending an incomplete-looking object.
    let e = ev(AgentEventKind::ToolCall {
        tool_name: "edit".into(),
        tool_use_id: Some("tc-delta".into()),
        parent_tool_use_id: None,
        input: json!({"partial": true, "chunk": "first_half"}),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert_eq!(input["partial"], true);
        assert_eq!(input["chunk"], "first_half");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn tool_call_end_event() {
    // A ToolResult signals the end of a tool call.
    let e = ev(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tc-end".into()),
        output: json!({"status": "done"}),
        is_error: false,
    });
    if let AgentEventKind::ToolResult {
        tool_name,
        is_error,
        ..
    } = &e.kind
    {
        assert_eq!(tool_name, "bash");
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn tool_result_event_with_output() {
    let output = json!({"stdout": "Hello, world!", "exit_code": 0});
    let e = ev(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("tu-out".into()),
        output: output.clone(),
        is_error: false,
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::ToolResult { output: rt_out, .. } = &rt.kind {
        assert_eq!(rt_out, &output);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn thinking_delta_via_assistant_delta() {
    // ABP models thinking as AssistantDelta with ext metadata.
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), json!(true));
    let e = AgentEvent {
        ts: now(),
        kind: AgentEventKind::AssistantDelta {
            text: "Let me consider...".into(),
        },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert!(rt.ext.as_ref().unwrap()["thinking"].as_bool().unwrap());
    assert_eq!(rt.text_content(), Some("Let me consider..."));
}

#[test]
fn error_event_with_error_info() {
    let e = ev(AgentEventKind::Error {
        message: "rate limit exceeded".into(),
        error_code: Some(abp_error::ErrorCode::BackendRateLimited),
    });
    let rt = roundtrip(&e);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &rt.kind
    {
        assert_eq!(message, "rate limit exceeded");
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendRateLimited));
    } else {
        panic!("expected Error");
    }
}

#[test]
fn metadata_event_via_ext() {
    // Metadata can be attached to any event via ext.
    let mut ext = BTreeMap::new();
    ext.insert("model".into(), json!("claude-3.5-sonnet"));
    ext.insert("request_id".into(), json!("req-abc-123"));
    let e = AgentEvent {
        ts: now(),
        kind: AgentEventKind::RunStarted {
            message: "starting with metadata".into(),
        },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    let rt_ext = rt.ext.unwrap();
    assert_eq!(rt_ext["model"], "claude-3.5-sonnet");
    assert_eq!(rt_ext["request_id"], "req-abc-123");
}

#[test]
fn custom_event_types_via_ext_fields() {
    // Custom/vendor events can be represented with ext.
    let mut ext = BTreeMap::new();
    ext.insert("custom_type".into(), json!("vendor_specific_event"));
    ext.insert("payload".into(), json!({"vendor": "acme", "code": 42}));
    let e = AgentEvent {
        ts: now(),
        kind: AgentEventKind::Warning {
            message: "custom event".into(),
        },
        ext: Some(ext),
    };
    let rt = roundtrip(&e);
    assert_eq!(
        rt.ext.as_ref().unwrap()["custom_type"],
        "vendor_specific_event"
    );
    assert_eq!(rt.ext.as_ref().unwrap()["payload"]["code"], 42);
}

#[test]
fn each_event_kind_serializes_with_correct_type_discriminator() {
    let cases: Vec<(&str, AgentEventKind)> = vec![
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
    for (expected_tag, kind) in cases {
        let e = ev(kind);
        let v: Value = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_tag,
            "wrong tag for {expected_tag}"
        );
    }
}

#[test]
fn event_deserialization_from_json() {
    let raw = json!({
        "ts": "2025-06-01T12:00:00Z",
        "type": "assistant_message",
        "text": "deserialized"
    });
    let e: AgentEvent = serde_json::from_value(raw).unwrap();
    assert!(matches!(
        e.kind,
        AgentEventKind::AssistantMessage { ref text } if text == "deserialized"
    ));
}

#[test]
fn event_roundtrip_through_serde_all_variants() {
    let events = vec![
        ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        ev(AgentEventKind::AssistantDelta { text: "hi".into() }),
        ev(AgentEventKind::ToolCall {
            tool_name: "f".into(),
            tool_use_id: Some("id".into()),
            parent_tool_use_id: None,
            input: json!({"a": 1}),
        }),
        ev(AgentEventKind::ToolResult {
            tool_name: "f".into(),
            tool_use_id: Some("id".into()),
            output: json!("ok"),
            is_error: false,
        }),
        ev(AgentEventKind::Error {
            message: "fail".into(),
            error_code: None,
        }),
    ];
    for e in &events {
        let rt = roundtrip(e);
        let orig = serde_json::to_value(e).unwrap();
        let rted = serde_json::to_value(&rt).unwrap();
        assert_eq!(orig, rted);
    }
}

#[test]
fn event_debug_formatting() {
    let e = ev(AgentEventKind::AssistantMessage {
        text: "debug".into(),
    });
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("AssistantMessage"));
    assert!(dbg.contains("debug"));
}

#[test]
fn empty_content_events() {
    let e1 = ev(AgentEventKind::AssistantDelta {
        text: String::new(),
    });
    let e2 = ev(AgentEventKind::AssistantMessage {
        text: String::new(),
    });
    let rt1 = roundtrip(&e1);
    let rt2 = roundtrip(&e2);
    assert_eq!(rt1.text_content(), Some(""));
    assert_eq!(rt2.text_content(), Some(""));
}

#[test]
fn large_content_events() {
    let big = "x".repeat(500_000);
    let e = ev(AgentEventKind::AssistantMessage { text: big.clone() });
    let rt = roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text.len(), 500_000);
        assert_eq!(text, &big);
    } else {
        panic!("expected AssistantMessage");
    }
}

// =========================================================================
// b) Stream behavior (10 tests)
// =========================================================================

#[test]
fn stream_buffer_accepts_events_up_to_capacity() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert_eq!(buf.len(), 3);
    assert!(buf.is_full());
}

#[test]
fn stream_buffer_drops_oldest_when_full() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c")); // "a" should be evicted
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(2);
    assert_eq!(recent[0].text_content(), Some("b"));
    assert_eq!(recent[1].text_content(), Some("c"));
}

#[test]
fn stream_metrics_tracks_event_counts_by_type() {
    let mut m = StreamMetrics::new();
    m.record_event(&ev(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    m.record_event(&delta("tok1"));
    m.record_event(&delta("tok2"));
    m.record_event(&ev(AgentEventKind::RunCompleted {
        message: "done".into(),
    }));
    assert_eq!(m.event_count(), 4);
    let counts = m.event_type_counts();
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["run_completed"], 1);
}

#[test]
fn stream_metrics_reports_total_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello")); // 5 bytes
    m.record_event(&delta("world!")); // 6 bytes
    m.record_event(&ev(AgentEventKind::RunStarted {
        message: "not counted".into(),
    }));
    assert_eq!(m.total_bytes(), 11);
}

#[tokio::test]
async fn stream_tee_broadcasts_to_multiple_receivers() {
    let (tx1, mut rx1) = tokio::sync::mpsc::channel(16);
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    let e = delta("broadcast");
    tee.send(&e).await.unwrap();

    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert_eq!(r1.text_content(), Some("broadcast"));
    assert_eq!(r2.text_content(), Some("broadcast"));
}

#[tokio::test]
async fn stream_tee_handles_closed_receivers() {
    let (tx1, rx1) = tokio::sync::mpsc::channel(16);
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    // Drop rx1 so its sender becomes closed.
    drop(rx1);

    let e = delta("surviving");
    // Should still succeed because tx2 is alive.
    tee.send(&e).await.unwrap();
    let received = rx2.recv().await.unwrap();
    assert_eq!(received.text_content(), Some("surviving"));
}

#[test]
fn empty_stream_behavior() {
    let stream = EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
    assert!(stream.duration().is_none());
    assert!(stream.first_of_kind("assistant_delta").is_none());
}

#[test]
fn single_event_stream() {
    let e = ev(AgentEventKind::RunStarted {
        message: "solo".into(),
    });
    let stream = EventStream::new(vec![e]);
    assert_eq!(stream.len(), 1);
    assert!(!stream.is_empty());
    assert!(stream.duration().is_none()); // need >=2 for duration
}

#[test]
fn rapid_event_succession() {
    // All events at the same timestamp should still be countable and ordered.
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| {
            ev_at(
                AgentEventKind::AssistantDelta {
                    text: format!("t{i}"),
                },
                ts,
            )
        })
        .collect();
    let stream = EventStream::new(events);
    assert_eq!(stream.len(), 100);
    let first = stream.first_of_kind("assistant_delta").unwrap();
    assert_eq!(first.text_content(), Some("t0"));
}

#[test]
fn event_ordering_preserved_through_buffer() {
    let mut buf = StreamBuffer::new(5);
    for i in 0..5 {
        buf.push(delta(&format!("ev{i}")));
    }
    let recent = buf.recent(5);
    for (i, e) in recent.iter().enumerate() {
        assert_eq!(e.text_content(), Some(format!("ev{i}").as_str()));
    }
}

// =========================================================================
// c) Event-to-protocol mapping (10 tests)
// =========================================================================

#[test]
fn agent_event_wraps_in_envelope_correctly() {
    let e = ev(AgentEventKind::AssistantDelta { text: "hi".into() });
    let ref_id = make_ref_id();
    let envelope = Envelope::Event {
        ref_id: ref_id.clone(),
        event: e.clone(),
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    assert!(encoded.contains("\"t\":\"event\""));
    assert!(encoded.contains(&ref_id));
}

#[test]
fn event_envelope_has_correct_ref_id() {
    let ref_id = "run-abc-123".to_string();
    let e = ev(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let envelope = Envelope::Event {
        ref_id: ref_id.clone(),
        event: e,
    };
    let json_str = JsonlCodec::encode(&envelope).unwrap();
    let parsed: Value = serde_json::from_str(json_str.trim()).unwrap();
    assert_eq!(parsed["ref_id"], "run-abc-123");
    assert_eq!(parsed["t"], "event");
}

#[test]
fn multiple_events_create_valid_jsonl_stream() {
    let ref_id = make_ref_id();
    let events = vec![
        ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        ev(AgentEventKind::AssistantDelta { text: "hi".into() }),
        ev(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let mut buf = Vec::new();
    for e in &events {
        let envelope = Envelope::Event {
            ref_id: ref_id.clone(),
            event: e.clone(),
        };
        JsonlCodec::encode_to_writer(&mut buf, &envelope).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
    for d in &decoded {
        assert!(matches!(d, Envelope::Event { .. }));
    }
}

#[test]
fn error_events_create_fatal_envelope() {
    let ref_id = "run-err".to_string();
    let envelope = Envelope::Fatal {
        ref_id: Some(ref_id.clone()),
        error: "something went wrong".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let parsed: Value = serde_json::from_str(encoded.trim()).unwrap();
    assert_eq!(parsed["t"], "fatal");
    assert_eq!(parsed["error"], "something went wrong");
    assert_eq!(parsed["ref_id"], "run-err");
}

#[test]
fn completion_creates_final_envelope() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let ref_id = make_ref_id();
    let envelope = Envelope::Final {
        ref_id: ref_id.clone(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let parsed: Value = serde_json::from_str(encoded.trim()).unwrap();
    assert_eq!(parsed["t"], "final");
    assert_eq!(parsed["ref_id"], ref_id);
    assert!(parsed.get("receipt").is_some());
}

#[test]
fn event_sequence_validation_via_jsonl() {
    // A valid sequence: hello → run → event* → final
    let backend = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    let hello = Envelope::hello(backend, CapabilityManifest::new());
    let run_event = Envelope::Event {
        ref_id: "r1".into(),
        event: ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let delta_event = Envelope::Event {
        ref_id: "r1".into(),
        event: delta("token"),
    };
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let final_env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };

    let mut buf = Vec::new();
    for env in [&hello, &run_event, &delta_event, &final_env] {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }

    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 4);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Final { .. }));
}

#[test]
fn timing_information_in_events() {
    let t1 = Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 5).unwrap();
    let events = vec![
        ev_at(
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            t1,
        ),
        ev_at(
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            t2,
        ),
    ];
    let stream = EventStream::new(events);
    let dur = stream.duration().unwrap();
    assert_eq!(dur.as_secs(), 5);
}

#[test]
fn event_filtering_by_type_on_stream() {
    let events = vec![
        ev(AgentEventKind::RunStarted {
            message: "a".into(),
        }),
        ev(AgentEventKind::AssistantDelta { text: "b".into() }),
        ev(AgentEventKind::Warning {
            message: "w".into(),
        }),
        ev(AgentEventKind::AssistantDelta { text: "c".into() }),
        ev(AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        }),
        ev(AgentEventKind::RunCompleted {
            message: "d".into(),
        }),
    ];
    let stream = EventStream::new(events);
    let only_deltas = stream.by_kind("assistant_delta");
    assert_eq!(only_deltas.len(), 2);

    let filter = EventFilter::include_kinds(&["warning", "error"]);
    let issues = stream.filter(&filter);
    assert_eq!(issues.len(), 2);
}

#[test]
fn event_aggregation_summary() {
    let events = vec![
        ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        ev(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!(null),
        }),
        ev(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: None,
            output: json!("contents"),
            is_error: false,
        }),
        ev(AgentEventKind::AssistantMessage {
            text: "here's the file".into(),
        }),
        ev(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    let summary = agg.summary();
    assert_eq!(summary.total_events, 5);
    assert_eq!(summary.tool_calls, 1);
    assert_eq!(summary.total_text_chars, 15); // "here's the file"
    assert_eq!(summary.errors, 0);
}

#[test]
fn event_replay_capability() {
    // Record events, then replay them into a new aggregator.
    let original_events = [
        ev(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        ev(AgentEventKind::AssistantDelta { text: "a".into() }),
        ev(AgentEventKind::AssistantDelta { text: "b".into() }),
        ev(AgentEventKind::AssistantDelta { text: "c".into() }),
        ev(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    // Serialize all events to JSON lines.
    let serialized: Vec<String> = original_events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();

    // "Replay" — deserialize and aggregate.
    let replayed: Vec<AgentEvent> = serialized
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();

    let analytics = RunAnalytics::from_events(&replayed);
    assert!(analytics.is_successful());
    let summary = analytics.summary();
    assert_eq!(summary.total_events, 5);
    assert_eq!(summary.total_text_chars, 3); // "a" + "b" + "c"
}

// =========================================================================
// Extra tests for fuller coverage
// =========================================================================

#[test]
fn stream_buffer_drain_returns_in_order() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("first"));
    buf.push(delta("second"));
    buf.push(delta("third"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0].text_content(), Some("first"));
    assert_eq!(drained[2].text_content(), Some("third"));
    assert!(buf.is_empty());
}

#[test]
fn stream_metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("tok"));
    let summary = m.summary();
    let display = format!("{}", summary);
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 3"));
}

#[tokio::test]
async fn stream_tee_all_receivers_closed_returns_error() {
    let (tx1, rx1) = tokio::sync::mpsc::channel::<AgentEvent>(1);
    let (tx2, rx2) = tokio::sync::mpsc::channel::<AgentEvent>(1);
    let tee = StreamTee::new(vec![tx1, tx2]);
    drop(rx1);
    drop(rx2);
    let result = tee.send(&delta("nobody home")).await;
    assert!(result.is_err());
}

#[test]
fn fatal_envelope_with_error_code() {
    let envelope = Envelope::fatal_with_code(
        Some("run-1".into()),
        "quota exceeded",
        abp_error::ErrorCode::BackendRateLimited,
    );
    let encoded = JsonlCodec::encode(&envelope).unwrap();
    let parsed: Value = serde_json::from_str(encoded.trim()).unwrap();
    assert_eq!(parsed["t"], "fatal");
    assert_eq!(parsed["error"], "quota exceeded");
    assert!(parsed.get("error_code").is_some());
}

#[test]
fn event_ext_trait_methods() {
    let tool = ev(AgentEventKind::ToolCall {
        tool_name: "edit".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    });
    assert!(tool.is_tool_call());
    assert!(!tool.is_terminal());
    assert!(tool.text_content().is_none());

    let completed = ev(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(completed.is_terminal());
    assert!(!completed.is_tool_call());
}

#[test]
fn stream_buffer_recent_returns_fewer_than_requested() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(5);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].text_content(), Some("only"));
}
