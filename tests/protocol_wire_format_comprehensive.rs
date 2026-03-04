#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the JSONL wire protocol format.
//!
//! Covers: encoding, decoding, validation, error handling, streaming,
//! unicode, malformed JSON, empty lines, large payloads, binary data (base64),
//! and the full envelope sequence validation.
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder, WorkspaceMode,
};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn hello_env() -> Envelope {
    Envelope::hello(backend("test-sidecar"), CapabilityManifest::new())
}

fn hello_env_with_caps() -> Envelope {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    Envelope::hello(backend("cap-sidecar"), caps)
}

fn run_env(task: &str) -> (String, Envelope) {
    let wo = WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
}

fn event_env(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn event_delta_env(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.into() },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("test-sidecar")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn encode(env: &Envelope) -> String {
    JsonlCodec::encode(env).unwrap()
}

fn roundtrip(env: &Envelope) -> Envelope {
    let json = encode(env);
    JsonlCodec::decode(json.trim()).unwrap()
}

// ===========================================================================
// 1. JSONL encoding/decoding for all Envelope variants
// ===========================================================================

#[test]
fn encode_hello_produces_newline_terminated_json() {
    let line = encode(&hello_env());
    assert!(line.ends_with('\n'));
    assert!(line.contains(r#""t":"hello""#));
    assert!(line.contains(r#""contract_version":"abp/v0.1""#));
}

#[test]
fn roundtrip_hello() {
    let original = hello_env();
    let decoded = roundtrip(&original);
    assert!(matches!(
        decoded,
        Envelope::Hello {
            contract_version, ..
        } if contract_version == CONTRACT_VERSION
    ));
}

#[test]
fn roundtrip_hello_with_capabilities() {
    let original = hello_env_with_caps();
    let decoded = roundtrip(&original);
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::ToolRead));
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn roundtrip_hello_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        backend("pt-sidecar"),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn encode_run_contains_work_order() {
    let (id, env) = run_env("refactor auth");
    let line = encode(&env);
    assert!(line.contains(r#""t":"run""#));
    assert!(line.contains(&id));
    assert!(line.contains("refactor auth"));
}

#[test]
fn roundtrip_run() {
    let (_id, env) = run_env("fix bug #42");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "fix bug #42");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn encode_event_contains_ref_id() {
    let env = event_env("run-123", "hello world");
    let line = encode(&env);
    assert!(line.contains(r#""t":"event""#));
    assert!(line.contains(r#""ref_id":"run-123""#));
}

#[test]
fn roundtrip_event_assistant_message() {
    let env = event_env("run-1", "The answer is 42.");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { text } if text == "The answer is 42."
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_assistant_delta() {
    let env = event_delta_env("run-d", "tok");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantDelta { text } if text == "tok"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-tc".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/foo.rs"}),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_tool_result() {
    let env = Envelope::Event {
        ref_id: "run-tr".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "write_file".into(),
                tool_use_id: Some("tu-2".into()),
                output: serde_json::json!({"ok": true}),
                is_error: false,
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::ToolResult { is_error, .. } if !is_error
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_file_changed() {
    let env = Envelope::Event {
        ref_id: "run-fc".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added error handling".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::FileChanged { ref path, .. } if path == "src/main.rs"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_command_executed() {
    let env = Envelope::Event {
        ref_id: "run-ce".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok".into()),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::CommandExecuted {
                    exit_code: Some(0),
                    ..
                }
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_warning() {
    let env = Envelope::Event {
        ref_id: "run-w".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "disk space low".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::Warning { ref message } if message == "disk space low"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_error() {
    let env = Envelope::Event {
        ref_id: "run-e".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "compile failed".into(),
                error_code: None,
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::Error { ref message, .. } if message == "compile failed"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_run_started() {
    let env = Envelope::Event {
        ref_id: "run-rs".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting execution".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_run_completed() {
    let env = Envelope::Event {
        ref_id: "run-rc".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_final_contains_receipt() {
    let env = final_env("run-456");
    let line = encode(&env);
    assert!(line.contains(r#""t":"final""#));
    assert!(line.contains(r#""ref_id":"run-456""#));
}

#[test]
fn roundtrip_final() {
    let env = final_env("run-f");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-f");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn encode_fatal_with_ref_id() {
    let env = fatal_env(Some("run-789"), "out of memory");
    let line = encode(&env);
    assert!(line.contains(r#""t":"fatal""#));
    assert!(line.contains(r#""ref_id":"run-789""#));
    assert!(line.contains("out of memory"));
}

#[test]
fn encode_fatal_without_ref_id() {
    let env = fatal_env(None, "startup failed");
    let line = encode(&env);
    assert!(line.contains(r#""ref_id":null"#));
    assert!(line.contains("startup failed"));
}

#[test]
fn roundtrip_fatal_with_ref_id() {
    let env = fatal_env(Some("run-z"), "kaboom");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-z"));
            assert_eq!(error, "kaboom");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn roundtrip_fatal_without_ref_id() {
    let env = fatal_env(None, "no ref");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { ref_id, .. } => {
            assert!(ref_id.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 2. The `t` tag field discrimination
// ===========================================================================

#[test]
fn tag_field_is_t_not_type() {
    let line = encode(&hello_env());
    assert!(line.contains(r#""t":"hello""#));
    // AgentEventKind uses "type", but the outer envelope uses "t"
    let event = event_env("r1", "msg");
    let event_line = encode(&event);
    assert!(line.contains(r#""t":"#));
    // The inner event uses "type" for AgentEventKind
    assert!(event_line.contains(r#""type":"assistant_message""#));
}

#[test]
fn decode_raw_json_with_t_field() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"raw test"}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(
        decoded,
        Envelope::Fatal { error, .. } if error == "raw test"
    ));
}

#[test]
fn decode_fails_with_type_instead_of_t() {
    let raw = r#"{"type":"fatal","ref_id":null,"error":"wrong tag"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
}

#[test]
fn all_variants_use_snake_case_tag() {
    let tags = [
        (encode(&hello_env()), "hello"),
        (encode(&run_env("x").1), "run"),
        (encode(&event_env("r", "m")), "event"),
        (encode(&final_env("r")), "final"),
        (encode(&fatal_env(None, "e")), "fatal"),
    ];
    for (line, expected_tag) in &tags {
        let expected = format!(r#""t":"{}""#, expected_tag);
        assert!(
            line.contains(&expected),
            "expected tag {expected} in: {line}"
        );
    }
}

// ===========================================================================
// 3. Unicode handling in JSON payloads
// ===========================================================================

#[test]
fn unicode_in_assistant_message() {
    let env = event_env("r-uni", "こんにちは世界 🌍 — €100");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { text }
                    if text == "こんにちは世界 🌍 — €100"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn unicode_in_fatal_error() {
    let env = fatal_env(None, "错误: 内存不足 💥");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "错误: 内存不足 💥");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_in_backend_id() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar-日本語".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "sidecar-日本語");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn unicode_escape_sequences_in_raw_json() {
    // JSON unicode escapes should decode correctly
    let raw = r#"{"t":"fatal","ref_id":null,"error":"\u0048\u0065\u006C\u006C\u006F"}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "Hello"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn emoji_roundtrip() {
    let env = event_env("r-emoji", "🦀🔥✅❌⚡🎯");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { text } if text == "🦀🔥✅❌⚡🎯"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn zero_width_and_control_chars() {
    // Zero-width joiner and various control-ish characters
    let text = "a\u{200D}b\u{FEFF}c";
    let env = event_env("r-zw", text);
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { ref text } if text == "a\u{200D}b\u{FEFF}c"
            ));
        }
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// 4. Malformed JSON handling
// ===========================================================================

#[test]
fn decode_empty_string_fails() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_garbage_fails() {
    let result = JsonlCodec::decode("not json at all");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn decode_valid_json_but_wrong_shape() {
    let result = JsonlCodec::decode(r#"{"name": "hello", "value": 42}"#);
    assert!(result.is_err());
}

#[test]
fn decode_missing_t_field() {
    let result = JsonlCodec::decode(r#"{"ref_id": "x", "error": "boom"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_unknown_t_value() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_variant","data":"x"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_truncated_json() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"err"#);
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn decode_json_array_not_object() {
    let result = JsonlCodec::decode(r#"[1, 2, 3]"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_primitive() {
    assert!(JsonlCodec::decode("42").is_err());
    assert!(JsonlCodec::decode("true").is_err());
    assert!(JsonlCodec::decode("null").is_err());
    assert!(JsonlCodec::decode(r#""just a string""#).is_err());
}

#[test]
fn decode_hello_missing_required_fields() {
    // Has correct t but missing backend/contract_version/capabilities
    let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_run_missing_work_order() {
    let result = JsonlCodec::decode(r#"{"t":"run","id":"x"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_event_missing_event_payload() {
    let result = JsonlCodec::decode(r#"{"t":"event","ref_id":"x"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_final_missing_receipt() {
    let result = JsonlCodec::decode(r#"{"t":"final","ref_id":"x"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_extra_fields_are_tolerated() {
    // serde default behavior: unknown fields are ignored
    let raw = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored"}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn decode_duplicate_t_field_is_rejected() {
    // serde tagged enums reject duplicate discriminator keys
    let raw = r#"{"t":"hello","t":"fatal","ref_id":null,"error":"dup"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
}

// ===========================================================================
// 5. Empty line handling
// ===========================================================================

#[test]
fn decode_stream_skips_empty_lines() {
    let fatal = encode(&fatal_env(None, "err1"));
    let input = format!("\n\n{}\n\n", fatal.trim());
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_stream_skips_whitespace_only_lines() {
    let fatal = encode(&fatal_env(None, "err2"));
    let input = format!("   \n  \t  \n{}\n  \n", fatal.trim());
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn streaming_codec_skips_blank_lines() {
    let fatal1 = encode(&fatal_env(None, "a"));
    let fatal2 = encode(&fatal_env(None, "b"));
    let input = format!("{}\n\n{}", fatal1.trim(), fatal2.trim());
    let results = StreamingCodec::decode_batch(&input);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(Result::is_ok));
}

#[test]
fn streaming_codec_line_count_ignores_blanks() {
    let input = "\n\nline1\n\nline2\n\n";
    assert_eq!(StreamingCodec::line_count(input), 2);
}

#[test]
fn stream_parser_skips_empty_lines() {
    let mut parser = StreamParser::new();
    let fatal = encode(&fatal_env(None, "sp-empty"));
    let input = format!("\n\n{}", fatal);
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// ===========================================================================
// 6. Very large payloads
// ===========================================================================

#[test]
fn large_assistant_message_roundtrip() {
    let big_text = "x".repeat(1_000_000); // 1MB of text
    let env = event_env("r-large", &big_text);
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { ref text } if text.len() == 1_000_000
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn large_payload_triggers_validation_warning() {
    let validator = EnvelopeValidator::new();
    // Create an event with a very large text (> 10MB)
    let huge_text = "y".repeat(11 * 1024 * 1024);
    let env = event_env("r-huge", &huge_text);
    let result = validator.validate(&env);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::LargePayload { .. }))
    );
}

#[test]
fn stream_parser_rejects_line_exceeding_max_len() {
    let mut parser = StreamParser::with_max_line_len(100);
    let long_line = format!(
        r#"{{"t":"fatal","ref_id":null,"error":"{}"}}"#,
        "z".repeat(200)
    );
    let input = format!("{long_line}\n");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(matches!(
        &results[0],
        Err(ProtocolError::Violation(msg)) if msg.contains("exceeds maximum")
    ));
}

#[test]
fn large_number_of_events_in_batch() {
    let envelopes: Vec<Envelope> = (0..1000)
        .map(|i| fatal_env(None, &format!("error-{i}")))
        .collect();
    let batch = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 1000);
    assert!(decoded.iter().all(Result::is_ok));
}

// ===========================================================================
// 7. Binary data handling (base64 encoded)
// ===========================================================================

#[test]
fn base64_in_tool_call_input() {
    use serde_json::json;
    let b64_data = "SGVsbG8gV29ybGQh"; // "Hello World!" in base64
    let env = Envelope::Event {
        ref_id: "r-b64".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tu-b64".into()),
                parent_tool_use_id: None,
                input: json!({
                    "path": "/tmp/binary.dat",
                    "content_base64": b64_data
                }),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input["content_base64"], b64_data);
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn large_base64_payload_roundtrip() {
    use serde_json::json;
    // Simulate a large binary blob encoded as base64 (100KB)
    let large_b64: String = std::iter::repeat_n('A', 100_000).collect();
    let env = Envelope::Event {
        ref_id: "r-lb64".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-lb64".into()),
                output: json!({ "data": large_b64 }),
                is_error: false,
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert_eq!(output["data"].as_str().unwrap().len(), 100_000);
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn null_bytes_in_json_string_via_escape() {
    // JSON allows \u0000 in strings
    let raw = r#"{"t":"fatal","ref_id":null,"error":"before\u0000after"}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains('\0'));
            assert!(error.starts_with("before"));
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 8. Streaming decode from reader
// ===========================================================================

#[test]
fn decode_stream_multiple_envelopes() {
    let env1 = fatal_env(None, "err-a");
    let env2 = fatal_env(Some("r1"), "err-b");
    let env3 = fatal_env(None, "err-c");
    let input = format!("{}{}{}", encode(&env1), encode(&env2), encode(&env3));
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn decode_stream_mixed_valid_and_invalid() {
    let valid = encode(&fatal_env(None, "ok"));
    let input = format!("{}not json\n{}", valid, valid);
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}

#[test]
fn decode_stream_only_empty_lines() {
    let reader = BufReader::new("\n\n\n\n".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}

#[test]
fn stream_parser_partial_line_then_complete() {
    let mut parser = StreamParser::new();
    let line = encode(&fatal_env(None, "partial"));
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    // First chunk: incomplete line → no results
    let r1 = parser.push(&bytes[..mid]);
    assert!(r1.is_empty());
    assert!(!parser.is_empty());

    // Second chunk: completes the line
    let r2 = parser.push(&bytes[mid..]);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_multiple_lines_in_one_push() {
    let mut parser = StreamParser::new();
    let line1 = encode(&fatal_env(None, "multi-1"));
    let line2 = encode(&fatal_env(None, "multi-2"));
    let combined = format!("{line1}{line2}");
    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(Result::is_ok));
}

#[test]
fn stream_parser_finish_flushes_unterminated_line() {
    let mut parser = StreamParser::new();
    let line = encode(&fatal_env(None, "flush"));
    let trimmed = line.trim(); // remove trailing newline
    parser.push(trimmed.as_bytes());
    assert!(!parser.is_empty());

    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_reset_discards_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"incomplete data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_invalid_utf8() {
    let mut parser = StreamParser::new();
    // Invalid UTF-8 followed by newline
    let data: Vec<u8> = vec![0xFF, 0xFE, 0xFD, b'\n'];
    let results = parser.push(&data);
    assert_eq!(results.len(), 1);
    assert!(matches!(
        &results[0],
        Err(ProtocolError::Violation(msg)) if msg.contains("UTF-8")
    ));
}

#[test]
fn encode_to_writer_works() {
    let mut buf = Vec::new();
    let env = fatal_env(None, "writer-test");
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("writer-test"));
}

#[test]
fn encode_many_to_writer_works() {
    let mut buf = Vec::new();
    let envelopes = vec![
        fatal_env(None, "w1"),
        fatal_env(None, "w2"),
        fatal_env(None, "w3"),
    ];
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 3);
}

// ===========================================================================
// 9. Newline termination requirements
// ===========================================================================

#[test]
fn encode_always_appends_newline() {
    for env in &[
        hello_env(),
        run_env("task").1,
        event_env("r", "msg"),
        final_env("r"),
        fatal_env(None, "e"),
    ] {
        let line = encode(env);
        assert!(line.ends_with('\n'), "missing newline for {:?}", env);
        // Exactly one newline at the end
        assert!(!line.ends_with("\n\n"));
    }
}

#[test]
fn decode_works_with_and_without_trailing_newline() {
    let line = encode(&fatal_env(None, "nl-test"));
    // With newline
    let with_nl = JsonlCodec::decode(line.trim());
    assert!(with_nl.is_ok());
    // Without newline (trim removes it)
    let raw = line.trim_end_matches('\n');
    let without_nl = JsonlCodec::decode(raw);
    assert!(without_nl.is_ok());
}

#[test]
fn encode_produces_single_line() {
    let env = event_env("r-sl", "line one");
    let encoded = encode(&env);
    // Should be exactly one line (the newline at end)
    assert_eq!(
        encoded.chars().filter(|&c| c == '\n').count(),
        1,
        "encoded envelope should be a single newline-terminated line"
    );
}

#[test]
fn crlf_in_stream_handled() {
    // Windows-style line endings
    let line = encode(&fatal_env(None, "crlf"));
    let crlf_input = line.trim_end_matches('\n').to_string() + "\r\n";
    let reader = BufReader::new(crlf_input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    // decode_stream trims each line, so \r should be handled
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// ===========================================================================
// 10. Envelope validation – sequence: hello→run→event*→final/fatal
// ===========================================================================

#[test]
fn valid_sequence_hello_run_events_final() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("valid sequence");
    let sequence = vec![
        hello_env(),
        run,
        event_env(&run_id, "msg1"),
        event_env(&run_id, "msg2"),
        final_env(&run_id),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_fatal() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("fatal sequence");
    let sequence = vec![
        hello_env(),
        run,
        fatal_env(Some(&run_id), "something went wrong"),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_no_events_final() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("no events");
    let sequence = vec![hello_env(), run, final_env(&run_id)];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn empty_sequence_reports_missing_hello_and_terminal() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn missing_hello_detected() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("no hello");
    let sequence = vec![run, final_env(&run_id)];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn hello_not_first_detected() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("hello second");
    let sequence = vec![run, hello_env(), final_env(&run_id)];
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 }))
    );
}

#[test]
fn missing_terminal_detected() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("no terminal");
    let sequence = vec![hello_env(), run, event_env(&run_id, "msg")];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn multiple_terminals_detected() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("multi terminal");
    let sequence = vec![
        hello_env(),
        run,
        final_env(&run_id),
        fatal_env(Some(&run_id), "also fatal"),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn ref_id_mismatch_detected() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("ref mismatch");
    let sequence = vec![
        hello_env(),
        run,
        event_env("wrong-ref-id", "msg"),
        final_env(&run_id),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn event_before_run_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("event before run");
    let sequence = vec![
        hello_env(),
        event_env(&run_id, "early event"),
        run,
        final_env(&run_id),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn event_after_terminal_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let (run_id, run) = run_env("event after final");
    let sequence = vec![
        hello_env(),
        run,
        final_env(&run_id),
        event_env(&run_id, "late event"),
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

// ===========================================================================
// Single-envelope validation
// ===========================================================================

#[test]
fn validate_hello_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello_env());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validate_hello_empty_backend_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::hello(
        BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn validate_hello_invalid_contract_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "invalid-version".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn validate_hello_empty_contract_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "contract_version"
    )));
}

#[test]
fn validate_run_empty_id() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "".into(),
        work_order: wo,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "id"
    )));
}

#[test]
fn validate_run_empty_task() {
    let validator = EnvelopeValidator::new();
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "work_order.task"
    )));
}

#[test]
fn validate_event_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = event_env("", "msg");
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "ref_id"
    )));
}

#[test]
fn validate_final_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = final_env("");
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let validator = EnvelopeValidator::new();
    let env = fatal_env(Some("r"), "");
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "error"
    )));
}

#[test]
fn validate_fatal_missing_ref_id_warns() {
    let validator = EnvelopeValidator::new();
    let env = fatal_env(None, "some error");
    let result = validator.validate(&env);
    assert!(result.valid); // warning, not error
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "ref_id"
    )));
}

#[test]
fn validate_hello_missing_optional_fields_warns() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field }
            if field == "backend.adapter_version"
    )));
}

// ===========================================================================
// StreamingCodec validate_jsonl
// ===========================================================================

#[test]
fn validate_jsonl_reports_bad_lines() {
    let good = encode(&fatal_env(None, "ok"))
        .trim_end_matches('\n')
        .to_string();
    let input = format!("{good}\nnot json\n{good}\n");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2); // 1-based line number
}

#[test]
fn validate_jsonl_all_good() {
    let line1 = encode(&fatal_env(None, "a"))
        .trim_end_matches('\n')
        .to_string();
    let line2 = encode(&fatal_env(None, "b"))
        .trim_end_matches('\n')
        .to_string();
    let input = format!("{line1}\n{line2}\n");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert!(errors.is_empty());
}

// ===========================================================================
// Version parsing and compatibility
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v1"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn version_compatibility() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
}

// ===========================================================================
// Extension field (ext) on AgentEvent
// ===========================================================================

#[test]
fn event_ext_field_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    );
    let env = Envelope::Event {
        ref_id: "r-ext".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_ext_field_none_not_serialized() {
    let env = event_env("r-no-ext", "msg");
    let json = encode(&env);
    // ext: None should be skipped via skip_serializing_if
    assert!(!json.contains("\"ext\""));
}

// ===========================================================================
// Envelope helper methods
// ===========================================================================

#[test]
fn fatal_with_code_helper() {
    let env = Envelope::fatal_with_code(
        Some("r-code".into()),
        "protocol error",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    match &env {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(
                *error_code,
                Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
            );
        }
        _ => panic!("expected Fatal"),
    }
    // error_code accessor
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn error_code_none_for_non_fatal() {
    assert!(hello_env().error_code().is_none());
    assert!(event_env("r", "m").error_code().is_none());
}

// ===========================================================================
// ProtocolError error_code accessor
// ===========================================================================

#[test]
fn protocol_error_codes() {
    let violation = ProtocolError::Violation("test".into());
    assert_eq!(
        violation.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        unexpected.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );

    let json_err = JsonlCodec::decode("bad").unwrap_err();
    assert!(json_err.error_code().is_none());
}

// ===========================================================================
// Edge cases: special characters in JSON strings
// ===========================================================================

#[test]
fn newlines_in_json_string_values_preserved() {
    let text_with_newlines = "line1\nline2\nline3";
    let env = event_env("r-nl", text_with_newlines);
    let json = encode(&env);
    // The newlines should be escaped as \n in JSON, not actual newlines
    assert_eq!(
        json.chars().filter(|&c| c == '\n').count(),
        1,
        "only the trailing newline should be literal"
    );
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { text }
                    if text == "line1\nline2\nline3"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn tabs_and_special_whitespace_in_strings() {
    let text = "col1\tcol2\t\tcol3";
    let env = event_env("r-tab", text);
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { text } if text == "col1\tcol2\t\tcol3"
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn backslashes_and_quotes_in_strings() {
    let text = r#"path: C:\Users\test "quoted""#;
    let env = event_env("r-bs", text);
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { ref text }
                    if text == r#"path: C:\Users\test "quoted""#
            ));
        }
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// Deterministic serialization (BTreeMap ordering)
// ===========================================================================

#[test]
fn capabilities_serialized_deterministically() {
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::ToolRead, SupportLevel::Native);
    caps1.insert(Capability::Streaming, SupportLevel::Emulated);
    caps1.insert(Capability::ToolWrite, SupportLevel::Native);

    // Insert in different order
    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::ToolWrite, SupportLevel::Native);
    caps2.insert(Capability::ToolRead, SupportLevel::Native);
    caps2.insert(Capability::Streaming, SupportLevel::Emulated);

    let env1 = Envelope::hello(backend("det"), caps1);
    let env2 = Envelope::hello(backend("det"), caps2);

    let json1 = encode(&env1);
    let json2 = encode(&env2);
    assert_eq!(
        json1, json2,
        "BTreeMap should produce deterministic ordering"
    );
}

// ===========================================================================
// Full protocol exchange simulation
// ===========================================================================

#[test]
fn full_protocol_exchange_through_stream() {
    let hello = hello_env();
    let (run_id, run) = run_env("full exchange test");
    let ev1 = event_env(&run_id, "starting...");
    let ev2 = event_delta_env(&run_id, "partial ");
    let ev3 = event_delta_env(&run_id, "response");
    let fin = final_env(&run_id);

    // Encode all to a single JSONL blob
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, run, ev1, ev2, ev3, fin]).unwrap();

    // Decode from stream
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 6);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Event { .. }));
    assert!(matches!(decoded[4], Envelope::Event { .. }));
    assert!(matches!(decoded[5], Envelope::Final { .. }));

    // Validate sequence
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&decoded);
    assert!(
        errors.is_empty(),
        "full exchange should be valid: {errors:?}"
    );
}

#[test]
fn full_exchange_via_stream_parser_chunked() {
    let hello = hello_env();
    let (run_id, run) = run_env("chunked exchange");
    let ev = event_env(&run_id, "hello");
    let fin = final_env(&run_id);

    let mut blob = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut blob, &[hello, run, ev, fin]).unwrap();

    // Feed in 10-byte chunks through the StreamParser
    let mut parser = StreamParser::new();
    let mut all_results = Vec::new();
    for chunk in blob.chunks(10) {
        all_results.extend(parser.push(chunk));
    }
    all_results.extend(parser.finish());

    let decoded: Vec<Envelope> = all_results
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// ===========================================================================
// CONTRACT_VERSION embedding
// ===========================================================================

#[test]
fn hello_embeds_contract_version() {
    let hello = hello_env();
    let json = encode(&hello);
    assert!(json.contains(CONTRACT_VERSION));
    match roundtrip(&hello) {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}
