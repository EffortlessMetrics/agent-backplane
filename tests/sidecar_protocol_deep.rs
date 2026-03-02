// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the sidecar JSONL protocol layer.
//!
//! Covers: envelope serde, JSONL parsing, ref_id correlation, contract version
//! validation, capability negotiation, fatal envelopes, edge cases, protocol
//! ordering, multi-run sequences, and backward compatibility.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

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

fn make_hello_with_caps(caps: CapabilityManifest) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "capable-sidecar".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        },
        caps,
    )
}

fn make_run() -> (String, Envelope) {
    let wo = WorkOrderBuilder::new("test task").build();
    let run_id = wo.id.to_string();
    let env = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    (run_id, env)
}

fn make_event(ref_id: &str, msg: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: msg.into() },
            ext: None,
        },
    }
}

fn make_delta_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.into() },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn roundtrip(env: &Envelope) -> Envelope {
    let line = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(line.trim()).unwrap()
}

// ===========================================================================
// 1. Envelope Serialization / Deserialization
// ===========================================================================

#[test]
fn hello_roundtrip() {
    let env = make_hello();
    let decoded = roundtrip(&env);
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_json_contains_tag() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn hello_json_contains_contract_version() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn run_roundtrip() {
    let (run_id, env) = make_run();
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, run_id);
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_json_contains_tag() {
    let (_, env) = make_run();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn event_roundtrip() {
    let env = make_event("run-1", "hello world");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "hello world"),
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_json_contains_tag() {
    let env = make_event("run-1", "msg");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn final_roundtrip() {
    let env = make_final("run-1");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_json_contains_tag() {
    let env = make_final("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn fatal_roundtrip_with_ref_id() {
    let env = make_fatal(Some("run-1"), "crash");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "crash");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_roundtrip_without_ref_id() {
    let env = make_fatal(None, "early failure");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_json_contains_tag() {
    let env = make_fatal(None, "oops");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

// ===========================================================================
// 2. JSONL Line Parsing
// ===========================================================================

#[test]
fn jsonl_single_line() {
    let env = make_fatal(None, "err");
    let line = JsonlCodec::encode(&env).unwrap();
    let reader = BufReader::new(line.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn jsonl_multi_line() {
    let mut buf = String::new();
    for i in 0..5 {
        let env = make_fatal(None, &format!("err-{i}"));
        buf.push_str(&JsonlCodec::encode(&env).unwrap());
    }
    let reader = BufReader::new(buf.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 5);
}

#[test]
fn jsonl_empty_lines_skipped() {
    let env = make_fatal(None, "err");
    let line = JsonlCodec::encode(&env).unwrap();
    let input = format!("\n\n{line}\n\n{line}\n");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn jsonl_whitespace_only_lines_skipped() {
    let env = make_fatal(None, "e");
    let line = JsonlCodec::encode(&env).unwrap();
    let input = format!("   \n\t\n{line}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn jsonl_decode_invalid_json_is_error() {
    let err = JsonlCodec::decode("not json at all").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn jsonl_decode_empty_string_is_error() {
    let err = JsonlCodec::decode("").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn jsonl_encode_ends_with_newline() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
    assert_eq!(json.matches('\n').count(), 1);
}

#[test]
fn stream_parser_partial_lines() {
    let env = make_fatal(None, "boom");
    let line = JsonlCodec::encode(&env).unwrap();
    let bytes = line.as_bytes();
    let (first, second) = bytes.split_at(bytes.len() / 2);

    let mut parser = StreamParser::new();
    let r1 = parser.push(first);
    assert!(r1.is_empty(), "partial line should not yield results");

    let r2 = parser.push(second);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_push() {
    let mut buf = Vec::new();
    for _ in 0..3 {
        let env = make_fatal(None, "err");
        let line = JsonlCodec::encode(&env).unwrap();
        buf.extend_from_slice(line.as_bytes());
    }
    let mut parser = StreamParser::new();
    let results = parser.push(&buf);
    assert_eq!(results.len(), 3);
}

#[test]
fn stream_parser_finish_flushes_unterminated() {
    let env = make_fatal(None, "pending");
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim_end(); // no trailing newline

    let mut parser = StreamParser::new();
    let r1 = parser.push(trimmed.as_bytes());
    assert!(r1.is_empty());

    let r2 = parser.finish();
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_empty_lines_skipped() {
    let env = make_fatal(None, "ok");
    let line = JsonlCodec::encode(&env).unwrap();
    let input = format!("\n\n{line}\n\n");

    let mut parser = StreamParser::new();
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

// ===========================================================================
// 3. ref_id Correlation
// ===========================================================================

#[test]
fn event_ref_id_must_match_run_id() {
    let (run_id, run_env) = make_run();
    let good_event = make_event(&run_id, "ok");
    let bad_event = make_event("wrong-id", "bad");
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();

    // Valid sequence
    let errs = validator.validate_sequence(&[
        make_hello(),
        run_env.clone(),
        good_event,
        final_env.clone(),
    ]);
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "no ref_id mismatch expected"
    );

    // Mismatch
    let errs = validator.validate_sequence(&[make_hello(), run_env, bad_event, final_env]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "expected ref_id mismatch"
    );
}

#[test]
fn final_ref_id_must_match_run_id() {
    let (run_id, run_env) = make_run();
    let bad_final = make_final("other-run");

    let validator = EnvelopeValidator::new();
    let errs =
        validator.validate_sequence(&[make_hello(), run_env, make_event(&run_id, "m"), bad_final]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn fatal_with_matching_ref_id_no_mismatch() {
    let (run_id, run_env) = make_run();
    let fatal = make_fatal(Some(&run_id), "error");

    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[make_hello(), run_env, fatal]);
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn fatal_with_wrong_ref_id_is_mismatch() {
    let (_, run_env) = make_run();
    let fatal = make_fatal(Some("bad-ref"), "error");

    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[make_hello(), run_env, fatal]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// ===========================================================================
// 4. Contract Version Validation
// ===========================================================================

#[test]
fn valid_contract_version_in_hello() {
    let env = make_hello();
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn empty_contract_version_is_error() {
    let env = Envelope::Hello {
        contract_version: String::new(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn invalid_contract_version_format_is_error() {
    let env = Envelope::Hello {
        contract_version: "invalid".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
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
fn contract_version_v0_1_is_current() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn parse_version_roundtrips() {
    let (major, minor) = abp_protocol::parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn compatible_versions_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.2", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn invalid_version_string_returns_none() {
    assert!(abp_protocol::parse_version("").is_none());
    assert!(abp_protocol::parse_version("v0.1").is_none());
    assert!(abp_protocol::parse_version("abp/0.1").is_none());
    assert!(abp_protocol::parse_version("abp/vabc").is_none());
}

// ===========================================================================
// 5. Capability Negotiation in Hello
// ===========================================================================

#[test]
fn hello_with_empty_capabilities() {
    let env = make_hello();
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_native_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let env = make_hello_with_caps(caps);
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Emulated)
            ));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_many_capabilities_roundtrips() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::ToolGlob, SupportLevel::Native);
    caps.insert(Capability::ToolGrep, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Unsupported);

    let env = make_hello_with_caps(caps.clone());
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), caps.len());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_passthrough_mode_roundtrips() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    match roundtrip(&env) {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = make_hello();
    match roundtrip(&env) {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// 6. Fatal Envelopes with Error Codes
// ===========================================================================

#[test]
fn fatal_with_error_code_roundtrips() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "version mismatch");
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolVersionMismatch)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_without_error_code_omits_field() {
    let env = make_fatal(None, "generic error");
    let json = JsonlCodec::encode(&env).unwrap();
    // error_code uses skip_serializing_if = "Option::is_none"
    assert!(!json.contains("error_code"));
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::BackendCrashed);
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendCrashed));
}

#[test]
fn non_fatal_error_code_is_none() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_from_abp_error() {
    let abp_err = abp_error::AbpError::new(
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        "bad envelope",
    );
    let env = Envelope::fatal_from_abp_error(Some("run-x".into()), &abp_err);
    match env {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "bad envelope");
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_various_error_codes() {
    let codes = [
        abp_error::ErrorCode::BackendTimeout,
        abp_error::ErrorCode::PolicyDenied,
        abp_error::ErrorCode::CapabilityUnsupported,
        abp_error::ErrorCode::Internal,
    ];
    for code in codes {
        let env = Envelope::fatal_with_code(None, "test", code);
        let decoded = roundtrip(&env);
        assert_eq!(decoded.error_code(), Some(code));
    }
}

// ===========================================================================
// 7. Edge Cases
// ===========================================================================

#[test]
fn unicode_in_event_message() {
    let env = make_event("run-1", "こんにちは世界 🌍 مرحبا");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "こんにちは世界 🌍 مرحبا");
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn unicode_in_fatal_error() {
    let env = make_fatal(None, "Ошибка: файл не найден 🔥");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "Ошибка: файл не найден 🔥");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn empty_task_in_work_order() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(
        result.errors.iter().any(
            |e| matches!(e, ValidationError::EmptyField { field } if field == "work_order.task")
        )
    );
}

#[test]
fn empty_run_id_is_validation_error() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: String::new(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn empty_ref_id_in_event_is_error() {
    let env = make_event("", "msg");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn empty_ref_id_in_final_is_error() {
    let env = make_final("");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn empty_error_in_fatal_is_error() {
    let env = make_fatal(Some("run-1"), "");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn empty_backend_id_in_hello_is_error() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn null_ref_id_in_fatal_generates_warning() {
    let env = make_fatal(None, "err");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(result.valid); // warning, not error
    assert!(!result.warnings.is_empty());
}

#[test]
fn very_long_message_roundtrips() {
    let long_text = "a".repeat(100_000);
    let env = make_event("run-1", &long_text);
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 100_000),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_tool_call_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/foo.txt"}),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_tool_result_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!("file contents here"),
                is_error: false,
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(!is_error),
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_file_changed_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added main function".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, .. } => assert_eq!(path, "src/main.rs"),
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_command_executed_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("All tests passed".into()),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted { exit_code, .. } => {
                assert_eq!(exit_code, Some(0));
            }
            _ => panic!("expected CommandExecuted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor": "data"}),
    );

    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext.clone()),
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            assert_eq!(
                event.ext.unwrap()["raw_message"],
                serde_json::json!({"vendor": "data"})
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_without_ext_omits_field() {
    let env = make_event("run-1", "no ext");
    let json = JsonlCodec::encode(&env).unwrap();
    // ext uses skip_serializing_if = "Option::is_none"
    assert!(!json.contains("raw_message"));
}

#[test]
fn special_chars_in_json_escaped() {
    let env = make_event("run-1", "line1\nline2\ttab\"quote\\backslash");
    let json = JsonlCodec::encode(&env).unwrap();
    // Newlines in values must be escaped in JSONL
    assert!(!json[..json.len() - 1].contains('\n') || json.matches('\n').count() == 1);
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('"'));
                assert!(text.contains('\\'));
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// 8. Protocol Ordering Violations
// ===========================================================================

#[test]
fn missing_hello_is_sequence_error() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs =
        validator.validate_sequence(&[run_env, make_event(&run_id, "m"), make_final(&run_id)]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
}

#[test]
fn hello_not_first_is_sequence_error() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[run_env, make_hello(), make_final(&run_id)]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn event_before_run_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        make_hello(),
        make_event("run-1", "early"),
        make_fatal(Some("run-1"), "abort"),
    ]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents))
    );
}

#[test]
fn missing_terminal_is_error() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[make_hello(), run_env, make_event(&run_id, "m")]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn empty_sequence_has_missing_hello_and_terminal() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn multiple_terminals_is_error() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        make_hello(),
        run_env,
        make_final(&run_id),
        make_fatal(Some(&run_id), "also done"),
    ]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals))
    );
}

#[test]
fn valid_minimal_sequence_no_events() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[make_hello(), run_env, make_final(&run_id)]);
    assert!(errs.is_empty(), "expected no errors: {errs:?}");
}

#[test]
fn valid_sequence_with_events() {
    let (run_id, run_env) = make_run();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        make_hello(),
        run_env,
        make_event(&run_id, "e1"),
        make_event(&run_id, "e2"),
        make_event(&run_id, "e3"),
        make_final(&run_id),
    ]);
    assert!(errs.is_empty(), "expected no errors: {errs:?}");
}

#[test]
fn valid_hello_then_fatal_no_run() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[make_hello(), make_fatal(None, "startup error")]);
    // No run → no ref_id mismatch, and fatal is terminal
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// ===========================================================================
// 9. Multiple Sequential Runs
// ===========================================================================

#[test]
fn encode_two_full_sessions_as_jsonl() {
    let mut buf = String::new();

    // Session 1
    let hello = make_hello();
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    let (run_id_1, run_env_1) = make_run();
    buf.push_str(&JsonlCodec::encode(&run_env_1).unwrap());
    buf.push_str(&JsonlCodec::encode(&make_event(&run_id_1, "working")).unwrap());
    buf.push_str(&JsonlCodec::encode(&make_final(&run_id_1)).unwrap());

    // Session 2 (reuse connection - new run)
    let (run_id_2, run_env_2) = make_run();
    buf.push_str(&JsonlCodec::encode(&run_env_2).unwrap());
    buf.push_str(&JsonlCodec::encode(&make_event(&run_id_2, "working again")).unwrap());
    buf.push_str(&JsonlCodec::encode(&make_final(&run_id_2)).unwrap());

    let reader = BufReader::new(buf.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 7);

    // Verify types in order
    assert!(matches!(results[0], Envelope::Hello { .. }));
    assert!(matches!(results[1], Envelope::Run { .. }));
    assert!(matches!(results[2], Envelope::Event { .. }));
    assert!(matches!(results[3], Envelope::Final { .. }));
    assert!(matches!(results[4], Envelope::Run { .. }));
    assert!(matches!(results[5], Envelope::Event { .. }));
    assert!(matches!(results[6], Envelope::Final { .. }));
}

#[test]
fn session_with_many_events() {
    let (run_id, run_env) = make_run();
    let mut envelopes = vec![make_hello(), run_env];
    for i in 0..100 {
        envelopes.push(make_delta_event(&run_id, &format!("token-{i}")));
    }
    envelopes.push(make_final(&run_id));

    // All roundtrip through JSONL
    let mut buf = String::new();
    for env in &envelopes {
        buf.push_str(&JsonlCodec::encode(env).unwrap());
    }
    let reader = BufReader::new(buf.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), envelopes.len());
}

#[test]
fn stream_parser_two_sequential_runs() {
    let mut parser = StreamParser::new();

    // Run 1
    let hello = make_hello();
    let (run_id_1, run_env_1) = make_run();
    let final_1 = make_final(&run_id_1);

    let mut results = Vec::new();
    results.extend(parser.push(JsonlCodec::encode(&hello).unwrap().as_bytes()));
    results.extend(parser.push(JsonlCodec::encode(&run_env_1).unwrap().as_bytes()));
    results.extend(parser.push(JsonlCodec::encode(&final_1).unwrap().as_bytes()));
    assert_eq!(results.len(), 3);

    // Run 2
    let (run_id_2, run_env_2) = make_run();
    let event_2 = make_event(&run_id_2, "second run");
    let final_2 = make_final(&run_id_2);

    results.extend(parser.push(JsonlCodec::encode(&run_env_2).unwrap().as_bytes()));
    results.extend(parser.push(JsonlCodec::encode(&event_2).unwrap().as_bytes()));
    results.extend(parser.push(JsonlCodec::encode(&final_2).unwrap().as_bytes()));
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|r| r.is_ok()));
}

// ===========================================================================
// 10. Backward Compatibility / Unknown Fields
// ===========================================================================

#[test]
fn unknown_fields_in_hello_are_skipped() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped","unknown_future_field":"value"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn unknown_fields_in_fatal_are_skipped() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":42}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn unknown_fields_in_event_are_skipped() {
    let env = make_event("run-1", "msg");
    let mut json_val: serde_json::Value = serde_json::to_value(&env).unwrap();
    json_val["future_field"] = serde_json::json!("new_thing");
    let json_str = serde_json::to_string(&json_val).unwrap();
    let decoded = JsonlCodec::decode(&json_str).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn missing_optional_mode_defaults_to_mapped() {
    // mode has #[serde(default)], so omitting it should default to Mapped
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn explicit_null_ref_id_in_fatal_decodes() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"test"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn explicit_string_ref_id_in_fatal_decodes() {
    let json = r#"{"t":"fatal","ref_id":"abc","error":"test"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("abc")),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn wrong_tag_value_is_error() {
    let json = r#"{"t":"unknown_variant","data":123}"#;
    let err = JsonlCodec::decode(json).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn missing_tag_is_error() {
    let json = r#"{"ref_id":null,"error":"no tag"}"#;
    let err = JsonlCodec::decode(json).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn encode_to_writer_works() {
    let env = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"hello""#));
}

#[test]
fn encode_many_to_writer_works() {
    let envs = [make_hello(), make_fatal(None, "err")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 2);
}

// ===========================================================================
// Additional Edge Cases
// ===========================================================================

#[test]
fn delta_event_roundtrips() {
    let env = make_delta_event("run-1", "tok");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "tok"),
            _ => panic!("expected AssistantDelta"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn warning_event_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "rate limit approaching".into(),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => {
                assert_eq!(message, "rate limit approaching");
            }
            _ => panic!("expected Warning"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn error_event_roundtrips() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: Some(abp_error::ErrorCode::Internal),
            },
            ext: None,
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::Internal));
            }
            _ => panic!("expected Error"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn run_started_and_completed_events_roundtrip() {
    let start = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
    };
    let end = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    };
    let s = roundtrip(&start);
    let e = roundtrip(&end);
    assert!(matches!(s, Envelope::Event { .. }));
    assert!(matches!(e, Envelope::Event { .. }));
}

#[test]
fn protocol_error_error_code_for_violation() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_error_code_for_unexpected_message() {
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
fn protocol_error_display_formats() {
    let err1 = ProtocolError::Violation("bad".into());
    assert!(err1.to_string().contains("bad"));

    let err2 = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert!(err2.to_string().contains("hello"));
    assert!(err2.to_string().contains("run"));
}

#[test]
fn stream_parser_max_line_len_enforced() {
    let mut parser = StreamParser::with_max_line_len(50);
    let long_line = format!("{}\n", "x".repeat(100));
    let results = parser.push(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn codec_batch_encode_decode() {
    use abp_protocol::codec::StreamingCodec;

    let envs = vec![make_hello(), make_fatal(None, "e1"), make_fatal(None, "e2")];
    let batch = StreamingCodec::encode_batch(&envs);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn codec_line_count() {
    use abp_protocol::codec::StreamingCodec;

    let envs = vec![make_hello(), make_fatal(None, "e")];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 2);
}

#[test]
fn codec_validate_jsonl_detects_bad_lines() {
    use abp_protocol::codec::StreamingCodec;

    let input = format!(
        "{}\nnot-json\n{}\n",
        JsonlCodec::encode(&make_hello()).unwrap().trim(),
        JsonlCodec::encode(&make_fatal(None, "e")).unwrap().trim(),
    );
    let errs = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].0, 2); // 1-based line number
}

#[test]
fn version_negotiation_same_version() {
    use abp_protocol::version::{ProtocolVersion, negotiate_version};

    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn version_negotiation_compatible_picks_min() {
    use abp_protocol::version::{ProtocolVersion, negotiate_version};

    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn version_negotiation_incompatible_fails() {
    use abp_protocol::version::{ProtocolVersion, VersionError, negotiate_version};

    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&v01, &v10).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}
