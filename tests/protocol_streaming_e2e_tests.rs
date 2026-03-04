// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive JSONL protocol streaming and parsing tests.
//!
//! Covers line parsing, envelope parsing, stream simulation, encoding,
//! and edge cases for the ABP protocol wire format.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// =========================================================================
// Helpers
// =========================================================================

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_run() -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn make_tool_call_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-001".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
    }
}

fn make_error_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something went wrong".into(),
                error_code: None,
            },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

fn make_fatal(ref_id: Option<&str>, msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: msg.into(),
        error_code: None,
    }
}

// =========================================================================
// 1. JSONL line parsing (10+ tests)
// =========================================================================

#[test]
fn jsonl_parse_valid_single_line() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
}

#[test]
fn jsonl_parse_multi_line_stream() {
    let input = r#"{"t":"fatal","ref_id":null,"error":"err1"}
{"t":"fatal","ref_id":null,"error":"err2"}
{"t":"fatal","ref_id":null,"error":"err3"}
"#;
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 3);
}

#[test]
fn jsonl_parse_empty_lines_between_valid() {
    let input = r#"{"t":"fatal","ref_id":null,"error":"first"}

{"t":"fatal","ref_id":null,"error":"second"}

"#;
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn jsonl_parse_trailing_newlines() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn jsonl_parse_unicode_content() {
    let env = make_event("run-1", "日本語テスト 🎉 émojis → λ");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("日本語テスト"));
                assert!(text.contains('🎉'));
                assert!(text.contains('λ'));
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn jsonl_parse_very_long_line() {
    let long_text = "x".repeat(1_100_000);
    let env = make_event("run-1", &long_text);
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.len() > 1_000_000);
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text.len(), 1_100_000);
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn jsonl_parse_whitespace_only_lines_skipped() {
    let input = "   \n\t\n  \n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(envelopes.is_empty());
}

#[test]
fn jsonl_parse_line_with_leading_trailing_whitespace() {
    let input = "  {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"trimmed\"}  \n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn jsonl_parse_single_line_no_trailing_newline() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"no newline"}"#;
    let decoded = JsonlCodec::decode(line).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn jsonl_streaming_codec_line_count() {
    let envelopes = vec![
        make_fatal(None, "a"),
        make_fatal(None, "b"),
        make_fatal(None, "c"),
    ];
    let batch = StreamingCodec::encode_batch(&envelopes);
    assert_eq!(StreamingCodec::line_count(&batch), 3);
}

#[test]
fn jsonl_streaming_codec_decode_batch() {
    let envelopes = vec![make_fatal(None, "one"), make_fatal(None, "two")];
    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 2);
    assert!(decoded.iter().all(|r| r.is_ok()));
}

#[test]
fn jsonl_parse_escaped_characters_in_string() {
    let env = make_event("run-1", "line1\nline2\ttab\"quote");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('"'));
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

// =========================================================================
// 2. Envelope parsing (10+ tests)
// =========================================================================

#[test]
fn envelope_parse_hello() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn envelope_parse_run() {
    let run = make_run();
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn envelope_parse_event() {
    let event = make_event("run-1", "hello world");
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { .. }
            ));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn envelope_parse_final() {
    let fin = make_final("run-1");
    let line = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn envelope_parse_fatal() {
    let fatal = make_fatal(Some("run-1"), "out of memory");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "out of memory");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn envelope_parse_fatal_without_ref_id() {
    let fatal = make_fatal(None, "early crash");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early crash");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn envelope_parse_unknown_type_errors() {
    let line = r#"{"t":"unknown_type","data":"test"}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn envelope_parse_missing_t_field_errors() {
    let line = r#"{"ref_id":"run-1","error":"no t field"}"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn envelope_parse_invalid_json_errors() {
    let line = "not valid json at all";
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn envelope_parse_truncated_json_errors() {
    let line = r#"{"t":"fatal","ref_id":"#;
    let result = JsonlCodec::decode(line);
    assert!(result.is_err());
}

#[test]
fn envelope_parse_empty_string_errors() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn envelope_parse_hello_with_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn envelope_parse_event_with_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"custom": "data"}));
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: Some(ext),
        },
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// =========================================================================
// 3. Stream simulation (10+ tests)
// =========================================================================

#[test]
fn stream_full_sidecar_conversation() {
    let wo = WorkOrderBuilder::new("do something").build();
    let run_id = wo.id.to_string();

    let hello = make_hello();
    let run = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    let event1 = make_event(&run_id, "thinking...");
    let event2 = make_event(&run_id, "done!");
    let fin = make_final(&run_id);

    let mut buf = Vec::new();
    for env in [&hello, &run, &event1, &event2, &fin] {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }

    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 5);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Run { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Event { .. }));
    assert!(matches!(envelopes[4], Envelope::Final { .. }));
}

#[test]
fn stream_interleaved_ref_ids() {
    let envelopes = vec![
        make_event("run-a", "msg for A"),
        make_event("run-b", "msg for B"),
        make_event("run-a", "more for A"),
        make_event("run-b", "more for B"),
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 4);
    let ref_ids: Vec<&str> = decoded
        .iter()
        .map(|e| match e {
            Envelope::Event { ref_id, .. } => ref_id.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(ref_ids, vec!["run-a", "run-b", "run-a", "run-b"]);
}

#[test]
fn stream_events_with_tool_calls() {
    let envelopes = vec![
        make_hello(),
        make_tool_call_event("run-1"),
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc-001".into()),
                    output: serde_json::json!("file contents"),
                    is_error: false,
                },
                ext: None,
            },
        },
    ];

    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 3);
    assert!(decoded.iter().all(|r| r.is_ok()));

    let env = decoded[1].as_ref().unwrap();
    match env {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::ToolCall { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn stream_events_with_errors() {
    let envelopes = vec![
        make_event("run-1", "starting"),
        make_error_event("run-1"),
        make_event("run-1", "recovered"),
    ];

    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 3);

    let err_env = decoded[1].as_ref().unwrap();
    match err_env {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::Error { .. }));
        }
        other => panic!("expected Error event, got {other:?}"),
    }
}

#[test]
fn stream_fatal_mid_stream() {
    let envelopes = vec![
        make_hello(),
        make_event("run-1", "working..."),
        make_fatal(Some("run-1"), "connection lost"),
    ];

    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 3);
    let last = decoded[2].as_ref().unwrap();
    assert!(matches!(last, Envelope::Fatal { .. }));
}

#[test]
fn stream_many_events() {
    let mut buf = Vec::new();
    for i in 0..100 {
        let env = make_event("run-1", &format!("event-{i}"));
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 100);
}

#[test]
fn stream_parser_incremental_feed() {
    let env = make_fatal(None, "test");
    let line = JsonlCodec::encode(&env).unwrap();
    let bytes = line.as_bytes();

    let mut parser = StreamParser::new();
    // Feed one byte at a time
    for (i, &b) in bytes.iter().enumerate() {
        let results = parser.feed(&[b]);
        if i < bytes.len() - 1 {
            assert!(results.is_empty(), "unexpected result at byte {i}");
        } else {
            assert_eq!(results.len(), 1, "expected one result at final byte");
            assert!(results[0].is_ok());
        }
    }
}

#[test]
fn stream_parser_two_envelopes_one_chunk() {
    let env1 = make_fatal(None, "first");
    let env2 = make_fatal(None, "second");
    let mut data = JsonlCodec::encode(&env1).unwrap();
    data.push_str(&JsonlCodec::encode(&env2).unwrap());

    let mut parser = StreamParser::new();
    let results = parser.feed(data.as_bytes());
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn stream_parser_finish_flushes_partial() {
    let env = make_fatal(None, "partial");
    let mut line = JsonlCodec::encode(&env).unwrap();
    // Remove trailing newline to make it a partial line
    line.pop(); // remove '\n'

    let mut parser = StreamParser::new();
    let results = parser.feed(line.as_bytes());
    assert!(results.is_empty());
    assert!(!parser.is_empty());

    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.feed(b"some incomplete ");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_simulation_file_changed_and_command_events() {
    let envelopes = vec![
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/main.rs".into(),
                    summary: "added main function".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::CommandExecuted {
                    command: "cargo test".into(),
                    exit_code: Some(0),
                    output_preview: Some("test result: ok".into()),
                },
                ext: None,
            },
        },
    ];

    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 2);
    assert!(decoded.iter().all(|r| r.is_ok()));
}

#[test]
fn stream_simulation_run_started_completed() {
    let envelopes = vec![
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "starting run".into(),
                },
                ext: None,
            },
        },
        make_event("run-1", "working"),
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "run finished".into(),
                },
                ext: None,
            },
        },
    ];

    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 3);
    assert!(decoded.iter().all(|r| r.is_ok()));
}

// =========================================================================
// 4. Encoding (10+ tests)
// =========================================================================

#[test]
fn encode_hello_envelope_valid_jsonl() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"hello""#));
    assert!(line.contains(r#""contract_version""#));
}

#[test]
fn encode_run_envelope_valid_jsonl() {
    let run = make_run();
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"run""#));
    assert!(line.contains(r#""work_order""#));
}

#[test]
fn encode_event_assistant_message_jsonl() {
    let event = make_event("run-1", "hello");
    let line = JsonlCodec::encode(&event).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"event""#));
    assert!(line.contains(r#""type":"assistant_message""#));
}

#[test]
fn encode_event_assistant_delta_jsonl() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""type":"assistant_delta""#));
}

#[test]
fn encode_event_tool_call_jsonl() {
    let env = make_tool_call_event("run-1");
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""type":"tool_call""#));
    assert!(line.contains(r#""tool_name":"read_file""#));
}

#[test]
fn encode_event_tool_result_jsonl() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-001".into()),
                output: serde_json::json!({"content": "hello"}),
                is_error: false,
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""type":"tool_result""#));
}

#[test]
fn encode_event_warning_jsonl() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "heads up".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""type":"warning""#));
}

#[test]
fn encode_final_envelope_jsonl() {
    let fin = make_final("run-1");
    let line = JsonlCodec::encode(&fin).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn encode_fatal_envelope_jsonl() {
    let fatal = make_fatal(Some("run-1"), "crash");
    let line = JsonlCodec::encode(&fatal).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn encode_roundtrip_hello() {
    let original = make_hello();
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(line, re_encoded);
}

#[test]
fn encode_roundtrip_fatal() {
    let original = make_fatal(Some("run-1"), "error message");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(line, re_encoded);
}

#[test]
fn encode_roundtrip_event() {
    let original = make_event("run-1", "test message");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    // Events contain timestamps that roundtrip, so the JSON should be identical
    assert_eq!(line, re_encoded);
}

#[test]
fn encode_deterministic_btreemap_ordering() {
    let mut caps = CapabilityManifest::new();
    caps.insert(
        abp_core::Capability::Streaming,
        abp_core::SupportLevel::Native,
    );
    caps.insert(
        abp_core::Capability::ToolRead,
        abp_core::SupportLevel::Native,
    );
    caps.insert(
        abp_core::Capability::ToolWrite,
        abp_core::SupportLevel::Emulated,
    );

    let hello = Envelope::hello(
        BackendIdentity {
            id: "caps-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps,
    );

    // Encode twice and verify identical output
    let line1 = JsonlCodec::encode(&hello).unwrap();
    let line2 = JsonlCodec::encode(&hello).unwrap();
    assert_eq!(line1, line2);
}

#[test]
fn encode_to_writer_produces_valid_output() {
    let env = make_fatal(None, "writer test");
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));

    let decoded = JsonlCodec::decode(output.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn encode_many_to_writer_produces_valid_output() {
    let envelopes = vec![
        make_fatal(None, "first"),
        make_fatal(None, "second"),
        make_fatal(None, "third"),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3);
}

// =========================================================================
// 5. Edge cases (10+ tests)
// =========================================================================

#[test]
fn edge_empty_stream() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(envelopes.is_empty());
}

#[test]
fn edge_empty_stream_parser() {
    let mut parser = StreamParser::new();
    let results = parser.feed(b"");
    assert!(results.is_empty());
    let results = parser.finish();
    assert!(results.is_empty());
}

#[test]
fn edge_very_large_payload_in_stream() {
    let big_text = "a".repeat(500_000);
    let env = make_event("run-1", &big_text);
    let line = JsonlCodec::encode(&env).unwrap();

    let mut parser = StreamParser::new();
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn edge_malformed_mid_stream() {
    let good1 = JsonlCodec::encode(&make_fatal(None, "ok")).unwrap();
    let bad = "this is not json\n";
    let good2 = JsonlCodec::encode(&make_fatal(None, "also ok")).unwrap();

    let input = format!("{good1}{bad}{good2}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn edge_malformed_mid_stream_parser() {
    let good1 = JsonlCodec::encode(&make_fatal(None, "ok")).unwrap();
    let bad = "broken json\n";
    let good2 = JsonlCodec::encode(&make_fatal(None, "also ok")).unwrap();

    let input = format!("{good1}{bad}{good2}");
    let mut parser = StreamParser::new();
    let results = parser.feed(input.as_bytes());

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn edge_binary_data_in_text_fields() {
    // Null bytes and control chars get escaped by JSON serialization
    let text_with_controls = "before\x01\x02\x03after";
    let env = make_event("run-1", text_with_controls);
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("before"));
                assert!(text.contains("after"));
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn edge_stream_parser_max_line_len() {
    let mut parser = StreamParser::with_max_line_len(50);
    let env = make_fatal(None, &"x".repeat(100));
    let line = JsonlCodec::encode(&env).unwrap();

    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => {
            assert!(msg.contains("exceeds maximum"));
        }
        other => panic!("expected Violation, got {other:?}"),
    }
}

#[test]
fn edge_stream_parser_invalid_utf8() {
    let mut parser = StreamParser::new();
    // Construct invalid UTF-8 followed by newline
    let mut data: Vec<u8> = vec![0xFF, 0xFE, 0x80];
    data.push(b'\n');

    let results = parser.feed(&data);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => {
            assert!(msg.contains("UTF-8"));
        }
        other => panic!("expected Violation, got {other:?}"),
    }
}

#[test]
fn edge_multiple_newlines_only() {
    let mut parser = StreamParser::new();
    let results = parser.feed(b"\n\n\n\n\n");
    assert!(results.is_empty());
}

#[test]
fn edge_crlf_line_endings() {
    let env = make_fatal(None, "crlf");
    let mut line = JsonlCodec::encode(&env).unwrap();
    // Replace \n with \r\n
    line = line.replace('\n', "\r\n");

    let reader = BufReader::new(line.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn edge_streaming_codec_validate_jsonl_reports_errors() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nnot json\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok2\"}\n";
    let errors = StreamingCodec::validate_jsonl(input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2); // 1-based line number
}

#[test]
fn edge_deeply_nested_json_in_event() {
    let mut value = serde_json::json!("leaf");
    for _ in 0..20 {
        value = serde_json::json!({"nested": value});
    }

    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "deep".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: value,
            },
            ext: None,
        },
    };

    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn edge_special_json_values_in_payload() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "special".into(),
                tool_use_id: None,
                output: serde_json::json!({
                    "null_val": null,
                    "bool_true": true,
                    "bool_false": false,
                    "integer": 42,
                    "float": 2.72,
                    "empty_string": "",
                    "empty_array": [],
                    "empty_object": {},
                }),
                is_error: false,
            },
            ext: None,
        },
    };

    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert!(output.get("null_val").unwrap().is_null());
                assert_eq!(output.get("integer").unwrap().as_i64(), Some(42));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn edge_stream_parser_buffered_len_accurate() {
    let mut parser = StreamParser::new();
    assert_eq!(parser.buffered_len(), 0);

    parser.feed(b"partial data");
    assert_eq!(parser.buffered_len(), 12);

    parser.feed(b" more");
    assert_eq!(parser.buffered_len(), 17);

    parser.reset();
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn edge_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "protocol error",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(decoded.error_code().is_some());
}

#[test]
fn edge_encode_all_event_kinds_roundtrip() {
    let ts = Utc::now();
    let event_kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("files"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "file.rs".into(),
            summary: "changed".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "echo hi".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
    ];

    for kind in event_kinds {
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts,
                kind,
                ext: None,
            },
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(line, re_encoded, "roundtrip failed for event");
    }
}
