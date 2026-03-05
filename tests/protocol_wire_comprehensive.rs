#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive test suite for the JSONL wire protocol.
//!
//! Validates envelope encoding/decoding, handshake sequences, and protocol
//! state machine transitions.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};

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

fn make_hello_passthrough() -> Envelope {
    Envelope::hello_with_mode(
        BackendIdentity {
            id: "passthrough-sidecar".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    )
}

fn make_run(id: &str) -> Envelope {
    let wo = WorkOrderBuilder::new("do something").build();
    Envelope::Run {
        id: id.to_string(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.to_string(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.to_string(),
        receipt,
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(|s| s.to_string()),
        error: error.to_string(),
        error_code: None,
    }
}

fn make_fatal_with_code(ref_id: Option<&str>, error: &str, code: abp_error::ErrorCode) -> Envelope {
    Envelope::fatal_with_code(ref_id.map(|s| s.to_string()), error, code)
}

// =========================================================================
// 1. Envelope discriminator — "t" field
// =========================================================================

#[test]
fn hello_envelope_has_t_discriminator() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#), "got: {json}");
}

#[test]
fn run_envelope_has_t_discriminator() {
    let env = make_run("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#), "got: {json}");
}

#[test]
fn event_envelope_has_t_discriminator() {
    let env = make_event("run-1", "hi");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#), "got: {json}");
}

#[test]
fn final_envelope_has_t_discriminator() {
    let env = make_final("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#), "got: {json}");
}

#[test]
fn fatal_envelope_has_t_discriminator() {
    let env = make_fatal(None, "boom");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#), "got: {json}");
}

#[test]
fn discriminator_field_is_t_not_type() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    // Should NOT have "type":"hello"
    assert!(!json.contains(r#""type":"hello""#));
    // SHOULD have "t":"hello"
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn decode_with_type_field_fails() {
    let json = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn decode_with_t_field_succeeds() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_ok());
}

// =========================================================================
// 2. Hello envelope encoding/decoding roundtrip
// =========================================================================

#[test]
fn hello_roundtrip() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_contract_version_present() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn hello_backend_id_preserved() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test-sidecar"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_passthrough_mode_roundtrip() {
    let env = make_hello_passthrough();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, backend, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
            assert_eq!(backend.id, "passthrough-sidecar");
            assert_eq!(backend.adapter_version.as_deref(), Some("0.1.0"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = make_hello();
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_empty_capabilities() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 3. Run envelope encoding/decoding roundtrip
// =========================================================================

#[test]
fn run_roundtrip() {
    let env = make_run("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-42");
            assert_eq!(work_order.task, "do something");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_work_order_task_preserved() {
    let env = make_run("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("do something"));
}

#[test]
fn run_id_preserved() {
    let env = make_run("custom-id-123");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { id, .. } => assert_eq!(id, "custom-id-123"),
        _ => panic!("expected Run"),
    }
}

// =========================================================================
// 4. Event envelope encoding/decoding roundtrip
// =========================================================================

#[test]
fn event_roundtrip() {
    let env = make_event("run-42", "Hello, world!");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-42");
            match event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, "Hello, world!");
                }
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_ref_id_preserved() {
    let env = make_event("ref-xyz", "msg");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "ref-xyz"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_assistant_delta() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "partial".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::AssistantDelta { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_call_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/file.txt"}),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "/tmp/file.txt");
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_result_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!({"content": "file contents"}),
                is_error: false,
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "added function");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("all tests pass".into()),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
            }
            _ => panic!("expected CommandExecuted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Warning {
                message: "budget low".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "budget low"),
            _ => panic!("expected Warning"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_error_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: Some(abp_error::ErrorCode::Internal),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert_eq!(message, "something broke");
                assert_eq!(error_code, Some(abp_error::ErrorCode::Internal));
            }
            _ => panic!("expected Error"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_run_started_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "Starting run".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_run_completed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "Done".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

// =========================================================================
// 5. Final envelope encoding/decoding roundtrip
// =========================================================================

#[test]
fn final_roundtrip() {
    let env = make_final("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_ref_id_preserved() {
    let env = make_final("my-ref");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "my-ref"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_receipt_backend_preserved() {
    let env = make_final("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.backend.id, "mock"),
        _ => panic!("expected Final"),
    }
}

// =========================================================================
// 6. Fatal envelope encoding/decoding roundtrip
// =========================================================================

#[test]
fn fatal_roundtrip_with_ref_id() {
    let env = make_fatal(Some("run-42"), "out of memory");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-42".to_string()));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_roundtrip_without_ref_id() {
    let env = make_fatal(None, "connection lost");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "connection lost");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_error_code_roundtrip() {
    let env = make_fatal_with_code(
        Some("r1"),
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_error_code_accessor() {
    let env = make_fatal_with_code(None, "timeout", abp_error::ErrorCode::BackendTimeout);
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

#[test]
fn non_fatal_error_code_is_none() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_from_abp_error() {
    let abp_err = abp_error::AbpError::new(
        abp_error::ErrorCode::ProtocolHandshakeFailed,
        "handshake timeout",
    );
    let env = Envelope::fatal_from_abp_error(Some("r1".to_string()), &abp_err);
    match &env {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "handshake timeout");
            assert_eq!(
                *error_code,
                Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// 7. Error code serialization (snake_case)
// =========================================================================

#[test]
fn error_code_serializes_as_snake_case() {
    let code = abp_error::ErrorCode::ProtocolInvalidEnvelope;
    assert_eq!(code.as_str(), "protocol_invalid_envelope");
}

#[test]
fn error_code_internal_is_snake_case() {
    let code = abp_error::ErrorCode::Internal;
    assert_eq!(code.as_str(), "internal");
}

#[test]
fn error_code_backend_timeout_is_snake_case() {
    let code = abp_error::ErrorCode::BackendTimeout;
    assert_eq!(code.as_str(), "backend_timeout");
}

#[test]
fn error_code_in_fatal_json_is_snake_case() {
    let env = make_fatal_with_code(None, "err", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("protocol_invalid_envelope"));
    // NOT SCREAMING_SNAKE_CASE
    assert!(!json.contains("PROTOCOL_INVALID_ENVELOPE"));
}

// =========================================================================
// 8. JSONL line-by-line parsing
// =========================================================================

#[test]
fn encode_appends_newline() {
    let env = make_fatal(None, "boom");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn encode_produces_single_line() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    // Exactly one newline at the end
    assert_eq!(json.matches('\n').count(), 1);
    assert!(json.ends_with('\n'));
}

#[test]
fn decode_handles_trailing_newline() {
    let env = make_fatal(None, "err");
    let json = JsonlCodec::encode(&env).unwrap();
    // decode should work on trimmed line
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn decode_handles_trailing_whitespace() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}  "#;
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

// =========================================================================
// 9. Multi-line JSONL streams
// =========================================================================

#[test]
fn decode_stream_multiple_lines() {
    let input = format!(
        "{}\n{}\n",
        JsonlCodec::encode(&make_fatal(None, "err1"))
            .unwrap()
            .trim(),
        JsonlCodec::encode(&make_fatal(None, "err2"))
            .unwrap()
            .trim(),
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_skips_blank_lines() {
    let input = format!(
        "\n\n{}\n\n{}\n\n",
        JsonlCodec::encode(&make_fatal(None, "err1"))
            .unwrap()
            .trim(),
        JsonlCodec::encode(&make_fatal(None, "err2"))
            .unwrap()
            .trim(),
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn decode_stream_only_blank_lines() {
    let reader = BufReader::new("\n\n\n".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn encode_to_writer_works() {
    let env = make_fatal(None, "boom");
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("boom"));
}

#[test]
fn encode_many_to_writer_works() {
    let envs = vec![
        make_fatal(None, "err1"),
        make_fatal(None, "err2"),
        make_fatal(None, "err3"),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 3);
    assert!(s.contains("err1"));
    assert!(s.contains("err3"));
}

// =========================================================================
// 10. StreamingCodec
// =========================================================================

#[test]
fn streaming_codec_encode_batch() {
    let envs = vec![make_fatal(None, "a"), make_fatal(None, "b")];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(batch.lines().count(), 2);
}

#[test]
fn streaming_codec_decode_batch() {
    let envs = vec![make_fatal(None, "a"), make_fatal(None, "b")];
    let batch = StreamingCodec::encode_batch(&envs);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn streaming_codec_line_count() {
    let envs = vec![
        make_fatal(None, "a"),
        make_fatal(None, "b"),
        make_fatal(None, "c"),
    ];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 3);
}

#[test]
fn streaming_codec_validate_jsonl_valid() {
    let envs = vec![make_fatal(None, "a")];
    let batch = StreamingCodec::encode_batch(&envs);
    let errors = StreamingCodec::validate_jsonl(&batch);
    assert!(errors.is_empty());
}

#[test]
fn streaming_codec_validate_jsonl_invalid() {
    let input = "not valid json\n";
    let errors = StreamingCodec::validate_jsonl(input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 1); // line 1
}

// =========================================================================
// 11. StreamParser (incremental)
// =========================================================================

#[test]
fn stream_parser_full_line() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_partial_line() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let bytes = line.as_bytes();
    let (first, second) = bytes.split_at(10);
    let r1 = parser.feed(first);
    assert!(r1.is_empty());
    let r2 = parser.feed(second);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_at_once() {
    let mut parser = StreamParser::new();
    let l1 = JsonlCodec::encode(&make_fatal(None, "a")).unwrap();
    let l2 = JsonlCodec::encode(&make_fatal(None, "b")).unwrap();
    let combined = format!("{}{}", l1, l2);
    let results = parser.feed(combined.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_skips_blank_lines() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let input = format!("\n\n{}\n\n", line.trim());
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_finish_flushes_unterminated() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let trimmed = line.trim();
    parser.feed(trimmed.as_bytes());
    assert!(parser.buffered_len() > 0);
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.feed(b"partial data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_max_line_len_enforced() {
    let mut parser = StreamParser::with_max_line_len(10);
    let long_line = format!(
        "{}",
        JsonlCodec::encode(&make_fatal(
            None,
            "a very long error message that exceeds the limit"
        ))
        .unwrap()
    );
    let results = parser.feed(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => {
            assert!(msg.contains("exceeds maximum"));
        }
        other => panic!("expected Violation, got {:?}", other),
    }
}

// =========================================================================
// 12. Protocol version parsing & compatibility
// =========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid_format() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/0.1"), None); // missing 'v'
    assert_eq!(parse_version("v0.1"), None); // missing 'abp/'
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_version_invalid_numbers() {
    assert_eq!(parse_version("abp/va.b"), None);
    assert_eq!(parse_version("abp/v1."), None);
    assert_eq!(parse_version("abp/v.1"), None);
}

#[test]
fn is_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn is_incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[test]
fn is_incompatible_invalid_versions() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

// =========================================================================
// 13. ProtocolVersion struct
// =========================================================================

#[test]
fn protocol_version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn protocol_version_parse_invalid() {
    assert!(ProtocolVersion::parse("invalid").is_err());
    assert!(ProtocolVersion::parse("abp/v").is_err());
    assert!(ProtocolVersion::parse("abp/va.1").is_err());
    assert!(ProtocolVersion::parse("abp/v1.b").is_err());
}

#[test]
fn protocol_version_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion { major: 3, minor: 7 };
    assert_eq!(format!("{}", v), "abp/v3.7");
}

#[test]
fn protocol_version_is_compatible() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01.is_compatible(&v02));
    assert!(!v01.is_compatible(&v10));
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01 < v02);
    assert!(v02 < v10);
}

// =========================================================================
// 14. VersionRange
// =========================================================================

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
}

#[test]
fn version_range_is_compatible() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

// =========================================================================
// 15. Version negotiation
// =========================================================================

#[test]
fn negotiate_version_same_version() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn negotiate_version_picks_minimum() {
    let local = ProtocolVersion { major: 0, minor: 2 };
    let remote = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result, remote);
}

#[test]
fn negotiate_version_incompatible() {
    let local = ProtocolVersion { major: 0, minor: 1 };
    let remote = ProtocolVersion { major: 1, minor: 0 };
    assert!(negotiate_version(&local, &remote).is_err());
}

// =========================================================================
// 16. ref_id correlation
// =========================================================================

#[test]
fn event_ref_id_matches_run_id() {
    let run = make_run("run-abc");
    let event = make_event("run-abc", "hi");
    match (&run, &event) {
        (Envelope::Run { id, .. }, Envelope::Event { ref_id, .. }) => {
            assert_eq!(id, ref_id);
        }
        _ => panic!("wrong variants"),
    }
}

#[test]
fn final_ref_id_matches_run_id() {
    let run = make_run("run-def");
    let fin = make_final("run-def");
    match (&run, &fin) {
        (Envelope::Run { id, .. }, Envelope::Final { ref_id, .. }) => {
            assert_eq!(id, ref_id);
        }
        _ => panic!("wrong variants"),
    }
}

#[test]
fn fatal_ref_id_matches_run_id() {
    let run = make_run("run-ghi");
    let fatal = make_fatal(Some("run-ghi"), "oops");
    match (&run, &fatal) {
        (Envelope::Run { id, .. }, Envelope::Fatal { ref_id, .. }) => {
            assert_eq!(Some(id.clone()), *ref_id);
        }
        _ => panic!("wrong variants"),
    }
}

// =========================================================================
// 17. Envelope validation (single envelope)
// =========================================================================

#[test]
fn validate_valid_hello() {
    let v = EnvelopeValidator::new();
    let env = make_hello();
    let result = v.validate(&env);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validate_hello_empty_backend_id() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn validate_hello_empty_contract_version() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "contract_version"
    )));
}

#[test]
fn validate_hello_invalid_version() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "garbage".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

#[test]
fn validate_hello_warns_missing_backend_version() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version"
    )));
}

#[test]
fn validate_run_empty_id() {
    let v = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_run_empty_task() {
    let v = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_event_empty_ref_id() {
    let v = EnvelopeValidator::new();
    let env = make_event("", "text");
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_final_empty_ref_id() {
    let v = EnvelopeValidator::new();
    let env = make_final("");
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let v = EnvelopeValidator::new();
    let env = make_fatal(Some("r1"), "");
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_warns_missing_ref_id() {
    let v = EnvelopeValidator::new();
    let env = make_fatal(None, "oops");
    let result = v.validate(&env);
    assert!(result.valid); // warning, not error
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "ref_id"
    )));
}

// =========================================================================
// 18. Handshake sequence validation
// =========================================================================

#[test]
fn sequence_valid_complete() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1", "hello"),
        make_final("r1"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.is_empty(), "got errors: {:?}", errors);
}

#[test]
fn sequence_missing_hello() {
    let v = EnvelopeValidator::new();
    let envs = vec![make_run("r1"), make_final("r1")];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn sequence_hello_not_first() {
    let v = EnvelopeValidator::new();
    let envs = vec![make_run("r1"), make_hello(), make_final("r1")];
    let errors = v.validate_sequence(&envs);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 })));
}

#[test]
fn sequence_missing_terminal() {
    let v = EnvelopeValidator::new();
    let envs = vec![make_hello(), make_run("r1"), make_event("r1", "hi")];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_multiple_terminals() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_fatal(Some("r1"), "oops"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn sequence_ref_id_mismatch_in_event() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("r1"),
        make_event("wrong-ref", "hi"),
        make_final("r1"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch {
            expected,
            found
        } if expected == "r1" && found == "wrong-ref"
    )));
}

#[test]
fn sequence_ref_id_mismatch_in_final() {
    let v = EnvelopeValidator::new();
    let envs = vec![make_hello(), make_run("r1"), make_final("wrong-ref")];
    let errors = v.validate_sequence(&envs);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn sequence_event_before_run() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_event("r1", "premature"),
        make_run("r1"),
        make_final("r1"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn sequence_empty() {
    let v = EnvelopeValidator::new();
    let envs: Vec<Envelope> = vec![];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_with_fatal_terminal() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("r1"), "crash"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.is_empty(), "got errors: {:?}", errors);
}

#[test]
fn sequence_many_events() {
    let v = EnvelopeValidator::new();
    let mut envs = vec![make_hello(), make_run("r1")];
    for i in 0..50 {
        envs.push(make_event("r1", &format!("event-{}", i)));
    }
    envs.push(make_final("r1"));
    let errors = v.validate_sequence(&envs);
    assert!(errors.is_empty(), "got errors: {:?}", errors);
}

// =========================================================================
// 19. Error handling for malformed envelopes
// =========================================================================

#[test]
fn decode_empty_string_fails() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_not_json_fails() {
    let result = JsonlCodec::decode("this is not json");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn decode_json_without_t_field_fails() {
    let result = JsonlCodec::decode(r#"{"foo":"bar"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_with_unknown_t_value_fails() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_hello_missing_required_fields_fails() {
    let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_run_missing_work_order_fails() {
    let result = JsonlCodec::decode(r#"{"t":"run","id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_event_missing_event_field_fails() {
    let result = JsonlCodec::decode(r#"{"t":"event","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_final_missing_receipt_fails() {
    let result = JsonlCodec::decode(r#"{"t":"final","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_array_fails() {
    let result = JsonlCodec::decode("[1,2,3]");
    assert!(result.is_err());
}

#[test]
fn decode_json_number_fails() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn decode_json_string_fails() {
    let result = JsonlCodec::decode(r#""hello""#);
    assert!(result.is_err());
}

#[test]
fn decode_json_null_fails() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn decode_json_boolean_fails() {
    let result = JsonlCodec::decode("true");
    assert!(result.is_err());
}

#[test]
fn decode_truncated_json_fails() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"er"#);
    assert!(result.is_err());
}

// =========================================================================
// 20. ProtocolError variants
// =========================================================================

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_error_abp_has_error_code() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
    let err = ProtocolError::from(abp_err);
    assert_eq!(err.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

#[test]
fn protocol_error_display() {
    let err = ProtocolError::Violation("bad things".into());
    let msg = format!("{}", err);
    assert!(msg.contains("bad things"));
}

#[test]
fn protocol_error_unexpected_message_display() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

// =========================================================================
// 21. Special characters and unicode
// =========================================================================

#[test]
fn unicode_in_fatal_error() {
    let env = make_fatal(None, "エラー: 失敗しました 🚫");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "エラー: 失敗しました 🚫"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_in_event_text() {
    let env = make_event("r1", "こんにちは世界 🌍");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "こんにちは世界 🌍");
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn special_chars_in_error_message() {
    let env = make_fatal(None, r#"error: "quotes" and \backslash and /forward"#);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("quotes"));
            assert!(error.contains("backslash"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn newline_in_error_message_roundtrips() {
    let env = make_fatal(None, "line1\nline2\nline3");
    let json = JsonlCodec::encode(&env).unwrap();
    // The newlines must be escaped in JSON so the JSONL line stays on one line
    let line_count = json.trim().lines().count();
    assert_eq!(line_count, 1, "should be a single JSONL line");
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "line1\nline2\nline3"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn tab_characters_roundtrip() {
    let env = make_fatal(None, "col1\tcol2\tcol3");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "col1\tcol2\tcol3"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn emoji_in_ref_id() {
    let env = make_event("run-🎉", "hooray");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-🎉"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn empty_string_fields_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("".into()),
        error: "e".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, Some("".into())),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn very_long_error_message() {
    let long_msg = "x".repeat(100_000);
    let env = make_fatal(None, &long_msg);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn null_bytes_in_json_string() {
    // Null bytes should be escaped by JSON serialization
    let env = make_fatal(None, "before\x00after");
    let result = JsonlCodec::encode(&env);
    // serde_json may or may not handle null bytes, but it shouldn't crash
    if let Ok(json) = result {
        // If it encoded, the decoded version should contain the same data
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { .. }));
    }
}

// =========================================================================
// 22. Complete run lifecycle
// =========================================================================

#[test]
fn lifecycle_hello_run_events_final() {
    let v = EnvelopeValidator::new();
    let run_id = "lifecycle-1";
    let envs = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id, "starting"),
        make_event(run_id, "processing"),
        make_event(run_id, "finishing"),
        make_final(run_id),
    ];
    // Encode all to JSONL
    let batch = StreamingCodec::encode_batch(&envs);
    // Decode
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|r| r.is_ok()));
    // Validate sequence
    let decoded: Vec<Envelope> = results.into_iter().map(|r| r.unwrap()).collect();
    let errors = v.validate_sequence(&decoded);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn lifecycle_hello_run_fatal() {
    let v = EnvelopeValidator::new();
    let run_id = "lifecycle-2";
    let envs = vec![
        make_hello(),
        make_run(run_id),
        make_fatal(Some(run_id), "backend crashed"),
    ];
    let batch = StreamingCodec::encode_batch(&envs);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 3);
    let decoded: Vec<Envelope> = results.into_iter().map(|r| r.unwrap()).collect();
    let errors = v.validate_sequence(&decoded);
    assert!(errors.is_empty(), "got: {:?}", errors);
}

#[test]
fn lifecycle_stream_parser_incremental() {
    let mut parser = StreamParser::new();
    let run_id = "lc-stream";
    let envs = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id, "working"),
        make_final(run_id),
    ];

    let mut all_results = Vec::new();
    for env in &envs {
        let line = JsonlCodec::encode(env).unwrap();
        // Feed byte by byte
        for byte in line.as_bytes() {
            let results = parser.feed(&[*byte]);
            all_results.extend(results);
        }
    }
    assert_eq!(all_results.len(), 4);
    assert!(all_results.iter().all(|r| r.is_ok()));
}

#[test]
fn lifecycle_roundtrip_all_envelope_types() {
    let run_id = "roundtrip-all";
    let envs = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id, "delta"),
        make_final(run_id),
    ];

    for env in &envs {
        let json = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        // Verify the discriminant matches
        match (env, &decoded) {
            (Envelope::Hello { .. }, Envelope::Hello { .. }) => {}
            (Envelope::Run { .. }, Envelope::Run { .. }) => {}
            (Envelope::Event { .. }, Envelope::Event { .. }) => {}
            (Envelope::Final { .. }, Envelope::Final { .. }) => {}
            (Envelope::Fatal { .. }, Envelope::Fatal { .. }) => {}
            (orig, got) => panic!("type mismatch: {:?} vs {:?}", orig, got),
        }
    }
}

// =========================================================================
// 23. Raw JSON structure tests
// =========================================================================

#[test]
fn hello_json_structure() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(val["t"], "hello");
    assert_eq!(val["contract_version"], CONTRACT_VERSION);
    assert_eq!(val["backend"]["id"], "test-sidecar");
    assert_eq!(val["mode"], "mapped");
}

#[test]
fn fatal_json_structure() {
    let env = make_fatal(Some("r1"), "err");
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(val["t"], "fatal");
    assert_eq!(val["ref_id"], "r1");
    assert_eq!(val["error"], "err");
}

#[test]
fn fatal_no_error_code_omits_field() {
    let env = make_fatal(None, "err");
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    // error_code has skip_serializing_if = "Option::is_none", so it should be absent
    assert!(val.get("error_code").is_none());
}

#[test]
fn fatal_with_error_code_includes_field() {
    let env = make_fatal_with_code(None, "err", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(val["error_code"], "protocol_invalid_envelope");
}

#[test]
fn run_json_has_id_and_work_order() {
    let env = make_run("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(val["t"], "run");
    assert_eq!(val["id"], "r1");
    assert!(val.get("work_order").is_some());
}

#[test]
fn event_json_has_ref_id_and_event() {
    let env = make_event("r1", "hello");
    let json = JsonlCodec::encode(&env).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(val["t"], "event");
    assert_eq!(val["ref_id"], "r1");
    assert!(val.get("event").is_some());
}

// =========================================================================
// 24. Decode from raw JSON strings
// =========================================================================

#[test]
fn decode_minimal_fatal() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_fatal_with_ref_id() {
    let json = r#"{"t":"fatal","ref_id":"r1","error":"boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, Some("r1".into())),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_fatal_with_error_code() {
    let json =
        r#"{"t":"fatal","ref_id":null,"error":"boom","error_code":"protocol_invalid_envelope"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// 25. Envelope size & validation warnings
// =========================================================================

#[test]
fn validate_normal_sized_envelope_no_large_payload_warning() {
    let v = EnvelopeValidator::new();
    let env = make_fatal(None, "small error");
    let result = v.validate(&env);
    assert!(!result
        .warnings
        .iter()
        .any(|w| matches!(w, ValidationWarning::LargePayload { .. })));
}

// =========================================================================
// 26. Sequence error display
// =========================================================================

#[test]
fn sequence_error_display_missing_hello() {
    let err = SequenceError::MissingHello;
    let msg = format!("{}", err);
    assert!(msg.contains("Hello"));
}

#[test]
fn sequence_error_display_missing_terminal() {
    let err = SequenceError::MissingTerminal;
    let msg = format!("{}", err);
    assert!(msg.contains("terminal"));
}

#[test]
fn sequence_error_display_hello_not_first() {
    let err = SequenceError::HelloNotFirst { position: 2 };
    let msg = format!("{}", err);
    assert!(msg.contains("2"));
}

#[test]
fn sequence_error_display_multiple_terminals() {
    let err = SequenceError::MultipleTerminals;
    let msg = format!("{}", err);
    assert!(msg.contains("multiple"));
}

#[test]
fn sequence_error_display_ref_id_mismatch() {
    let err = SequenceError::RefIdMismatch {
        expected: "r1".into(),
        found: "r2".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("r1"));
    assert!(msg.contains("r2"));
}

#[test]
fn sequence_error_display_out_of_order() {
    let err = SequenceError::OutOfOrderEvents;
    let msg = format!("{}", err);
    assert!(msg.contains("Event"));
}

// =========================================================================
// 27. Validation error display
// =========================================================================

#[test]
fn validation_error_display_missing_field() {
    let err = ValidationError::MissingField {
        field: "backend.id".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("backend.id"));
}

#[test]
fn validation_error_display_invalid_value() {
    let err = ValidationError::InvalidValue {
        field: "mode".into(),
        value: "bad".into(),
        expected: "mapped or passthrough".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("mode"));
    assert!(msg.contains("bad"));
}

#[test]
fn validation_error_display_invalid_version() {
    let err = ValidationError::InvalidVersion {
        version: "garbage".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("garbage"));
}

#[test]
fn validation_error_display_empty_field() {
    let err = ValidationError::EmptyField {
        field: "ref_id".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("ref_id"));
}

// =========================================================================
// 28. Validation warning display
// =========================================================================

#[test]
fn validation_warning_display_large_payload() {
    let w = ValidationWarning::LargePayload {
        size: 20_000_000,
        max_recommended: 10_000_000,
    };
    let msg = format!("{}", w);
    assert!(msg.contains("20000000"));
}

#[test]
fn validation_warning_display_missing_optional() {
    let w = ValidationWarning::MissingOptionalField {
        field: "backend.backend_version".into(),
    };
    let msg = format!("{}", w);
    assert!(msg.contains("backend.backend_version"));
}

// =========================================================================
// 29. Edge cases and additional coverage
// =========================================================================

#[test]
fn hello_mode_default_when_absent_in_json() {
    // When "mode" is absent in JSON, it should default to Mapped
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_passthrough_mode_in_json() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"passthrough"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn event_with_ext_field_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"custom": "data"}),
    );
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "with ext".into(),
            },
            ext: Some(ext),
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn multiple_roundtrips_produce_consistent_json() {
    let env = make_hello();
    let json1 = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json1.trim()).unwrap();
    let json2 = JsonlCodec::encode(&decoded).unwrap();
    // JSON should be identical after roundtrip
    assert_eq!(json1, json2);
}

#[test]
fn fatal_ref_id_mismatch_in_sequence() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("wrong-ref"), "crash"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn decode_stream_with_mixed_valid_invalid() {
    let valid = JsonlCodec::encode(&make_fatal(None, "ok")).unwrap();
    let input = format!("{}not valid json\n{}", valid, valid.trim());
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn stream_parser_invalid_utf8_produces_violation() {
    let mut parser = StreamParser::new();
    let invalid = [0xFF, 0xFE, b'\n'];
    let results = parser.feed(&invalid);
    assert_eq!(results.len(), 1);
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => assert!(msg.contains("UTF-8")),
        other => panic!("expected Violation, got {:?}", other),
    }
}

#[test]
fn encode_decode_cycle_100_envelopes() {
    for i in 0..100 {
        let env = make_fatal(Some(&format!("r-{}", i)), &format!("error-{}", i));
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id, Some(format!("r-{}", i)));
                assert_eq!(error, format!("error-{}", i));
            }
            _ => panic!("expected Fatal"),
        }
    }
}

#[test]
fn sequence_ref_id_mismatch_in_fatal_with_ref() {
    let v = EnvelopeValidator::new();
    let envs = vec![
        make_hello(),
        make_run("correct-ref"),
        make_fatal(Some("wrong-ref"), "fail"),
    ];
    let errors = v.validate_sequence(&envs);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch {
            expected,
            found
        } if expected == "correct-ref" && found == "wrong-ref"
    )));
}

#[test]
fn sequence_no_run_with_events_is_out_of_order() {
    let v = EnvelopeValidator::new();
    let envs = vec![make_hello(), make_event("r1", "hi"), make_final("r1")];
    let errors = v.validate_sequence(&envs);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn version_error_display_incompatible() {
    let local = ProtocolVersion { major: 0, minor: 1 };
    let remote = ProtocolVersion { major: 1, minor: 0 };
    let err = negotiate_version(&local, &remote).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("incompatible"));
}

#[test]
fn protocol_version_equality() {
    let v1 = ProtocolVersion { major: 0, minor: 1 };
    let v2 = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(v1, v2);
}

#[test]
fn protocol_version_inequality() {
    let v1 = ProtocolVersion { major: 0, minor: 1 };
    let v2 = ProtocolVersion { major: 0, minor: 2 };
    assert_ne!(v1, v2);
}

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn parse_current_contract_version() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}
