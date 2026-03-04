#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the sidecar protocol types, JSONL codec, envelope
//! construction, validation, builder patterns, sequence validation, routing,
//! version negotiation, stream parsing, state machine, frame validation,
//! compression, batch processing, and edge cases.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::*;
use abp_protocol::batch::{BatchProcessor, BatchRequest, BatchValidationError, MAX_BATCH_SIZE};
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::compress::{CompressionAlgorithm, CompressionStats, MessageCompressor};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{ProtocolVersion, VersionError, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use sidecar_kit::{
    Frame, FrameReader, FrameWriter, ProtocolPhase, ProtocolState, buf_reader_from_bytes,
    frame_to_json, json_to_frame, read_all_frames, validate_frame, write_frames,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("0.1.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "hello world".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/test".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn test_receipt(run_id: Uuid) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 42,
        },
        backend: test_identity(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::Value::Null,
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn test_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(test_identity(), test_capabilities())
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: test_work_order(),
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: test_event(),
    }
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: test_receipt(Uuid::nil()),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

// ===========================================================================
// 1. Envelope variant construction
// ===========================================================================

#[test]
fn hello_envelope_has_contract_version() {
    let env = make_hello();
    match &env {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, CONTRACT_VERSION),
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
fn hello_with_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        test_identity(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn run_envelope_contains_work_order() {
    let env = make_run("run-1");
    match &env {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-1");
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn event_envelope_carries_ref_id() {
    let env = make_event("run-42");
    match &env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn final_envelope_carries_receipt() {
    let env = make_final("run-x");
    match &env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-x");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_envelope_with_ref_id() {
    let env = make_fatal(Some("run-err"), "crash");
    match &env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
            ..
        } => {
            assert_eq!(ref_id, &Some("run-err".into()));
            assert_eq!(error, "crash");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_without_ref_id() {
    let env = make_fatal(None, "early failure");
    match &env {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_error_code_constructor() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "protocol broken",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn error_code_returns_none_for_non_fatal() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

// ===========================================================================
// 2. JSONL codec — encode / decode
// ===========================================================================

#[test]
fn encode_hello_ends_with_newline() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn encode_hello_contains_tag() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(line.contains(r#""t":"hello""#));
}

#[test]
fn decode_hello_roundtrip() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    let env = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn encode_run_contains_tag() {
    let line = JsonlCodec::encode(&make_run("r1")).unwrap();
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn decode_run_roundtrip() {
    let env = make_run("r1");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "r1");
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn encode_event_contains_tag() {
    let line = JsonlCodec::encode(&make_event("r1")).unwrap();
    assert!(line.contains(r#""t":"event""#));
}

#[test]
fn decode_event_roundtrip() {
    let env = make_event("r1");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_final_contains_tag() {
    let line = JsonlCodec::encode(&make_final("r1")).unwrap();
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn decode_final_roundtrip() {
    let env = make_final("r1");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn encode_fatal_contains_tag() {
    let line = JsonlCodec::encode(&make_fatal(Some("r1"), "boom")).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn decode_fatal_roundtrip() {
    let env = make_fatal(Some("r1"), "boom");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "boom");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_fatal_null_ref_id() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 3. Serde roundtrip for all event kinds
// ===========================================================================

fn roundtrip_event_kind(kind: AgentEventKind) {
    let event = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "rt".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn roundtrip_run_started() {
    roundtrip_event_kind(AgentEventKind::RunStarted {
        message: "go".into(),
    });
}

#[test]
fn roundtrip_run_completed() {
    roundtrip_event_kind(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
}

#[test]
fn roundtrip_assistant_delta() {
    roundtrip_event_kind(AgentEventKind::AssistantDelta { text: "tok".into() });
}

#[test]
fn roundtrip_assistant_message() {
    roundtrip_event_kind(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
}

#[test]
fn roundtrip_tool_call() {
    roundtrip_event_kind(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/foo"}),
    });
}

#[test]
fn roundtrip_tool_result() {
    roundtrip_event_kind(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        output: "content".into(),
        is_error: false,
    });
}

#[test]
fn roundtrip_file_changed() {
    roundtrip_event_kind(AgentEventKind::FileChanged {
        path: "/a.txt".into(),
        summary: "added".into(),
    });
}

#[test]
fn roundtrip_command_executed() {
    roundtrip_event_kind(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: Some("files".into()),
    });
}

#[test]
fn roundtrip_warning() {
    roundtrip_event_kind(AgentEventKind::Warning {
        message: "caution".into(),
    });
}

#[test]
fn roundtrip_error_event() {
    roundtrip_event_kind(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    });
}

// ===========================================================================
// 4. Decode stream
// ===========================================================================

#[test]
fn decode_stream_multiple_lines() {
    let env1 = JsonlCodec::encode(&make_fatal(None, "err1")).unwrap();
    let env2 = JsonlCodec::encode(&make_fatal(None, "err2")).unwrap();
    let input = format!("{env1}{env2}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn decode_stream_skips_blank_lines() {
    let env = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("\n\n{env}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(results.is_empty());
}

// ===========================================================================
// 5. Encode to writer / encode many
// ===========================================================================

#[test]
fn encode_to_writer_produces_newline() {
    let mut buf: Vec<u8> = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &make_hello()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"hello""#));
}

#[test]
fn encode_many_to_writer_multiple_envelopes() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let mut buf: Vec<u8> = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 4);
}

// ===========================================================================
// 6. Edge cases — malformed frames / missing fields
// ===========================================================================

#[test]
fn decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn decode_empty_string_returns_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_missing_tag_field_returns_error() {
    let result = JsonlCodec::decode(r#"{"ref_id":"x","error":"boom"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_unknown_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#);
    assert!(result.is_err());
}

#[test]
fn decode_run_missing_work_order_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"run","id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_event_missing_event_field_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"event","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_final_missing_receipt_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"final","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_fatal_missing_error_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_object_without_t_field() {
    let result = JsonlCodec::decode(r#"{"hello":"world"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_array_returns_error() {
    let result = JsonlCodec::decode(r#"[1,2,3]"#);
    assert!(result.is_err());
}

#[test]
fn decode_json_number_returns_error() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn decode_json_null_returns_error() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

// ===========================================================================
// 7. Hello handshake validation
// ===========================================================================

#[test]
fn validate_hello_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_hello());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validate_hello_empty_backend_id() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.contains(&ValidationError::EmptyField {
        field: "backend.id".into()
    }));
}

#[test]
fn validate_hello_empty_contract_version() {
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.contains(&ValidationError::EmptyField {
        field: "contract_version".into()
    }));
}

#[test]
fn validate_hello_invalid_contract_version() {
    let env = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::InvalidVersion { version } if version == "not-a-version")
    ));
}

#[test]
fn validate_hello_warns_missing_backend_version() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version"
    )));
}

// ===========================================================================
// 8. Run / Event / Final / Fatal validation
// ===========================================================================

#[test]
fn validate_run_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_run("r1"));
    assert!(result.valid);
}

#[test]
fn validate_run_empty_id() {
    let env = Envelope::Run {
        id: "".into(),
        work_order: test_work_order(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .contains(&ValidationError::EmptyField { field: "id".into() })
    );
}

#[test]
fn validate_run_empty_task() {
    let mut wo = test_work_order();
    wo.task = "".into();
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.contains(&ValidationError::EmptyField {
        field: "work_order.task".into()
    }));
}

#[test]
fn validate_event_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_event("r1"));
    assert!(result.valid);
}

#[test]
fn validate_event_empty_ref_id() {
    let env = Envelope::Event {
        ref_id: "".into(),
        event: test_event(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.contains(&ValidationError::EmptyField {
        field: "ref_id".into()
    }));
}

#[test]
fn validate_final_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_final("r1"));
    assert!(result.valid);
}

#[test]
fn validate_final_empty_ref_id() {
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt: test_receipt(Uuid::nil()),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_valid() {
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_fatal(Some("r1"), "boom"));
    assert!(result.valid);
}

#[test]
fn validate_fatal_empty_error() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "".into(),
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.contains(&ValidationError::EmptyField {
        field: "error".into()
    }));
}

#[test]
fn validate_fatal_without_ref_id_warns() {
    let env = make_fatal(None, "oops");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "ref_id"
    )));
}

// ===========================================================================
// 9. Sequence validation
// ===========================================================================

#[test]
fn valid_sequence_hello_run_event_final() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_fatal() {
    let seq = vec![make_hello(), make_run("r1"), make_fatal(Some("r1"), "err")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn sequence_missing_hello() {
    let seq = vec![make_run("r1"), make_event("r1"), make_final("r1")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn sequence_missing_terminal() {
    let seq = vec![make_hello(), make_run("r1"), make_event("r1")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_hello_not_first() {
    let seq = vec![
        make_run("r1"),
        make_hello(),
        make_event("r1"),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::HelloNotFirst { position: 1 }));
}

#[test]
fn sequence_multiple_terminals() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_fatal(Some("r1"), "extra"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn sequence_ref_id_mismatch() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r2"), // wrong ref_id
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
        if expected == "r1" && found == "r2"
    )));
}

#[test]
fn sequence_empty() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_out_of_order_event_before_run() {
    let seq = vec![
        make_hello(),
        make_event("r1"),
        make_run("r1"),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn sequence_multiple_events_valid() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_event("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected: {errors:?}");
}

#[test]
fn sequence_no_events_valid() {
    let seq = vec![make_hello(), make_run("r1"), make_final("r1")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

// ===========================================================================
// 10. ref_id correlation
// ===========================================================================

#[test]
fn ref_id_matches_across_event_and_final() {
    let run_id = "correlation-test";
    let event_env = make_event(run_id);
    let final_env = make_final(run_id);

    let event_ref = match &event_env {
        Envelope::Event { ref_id, .. } => ref_id.clone(),
        _ => panic!("expected Event"),
    };
    let final_ref = match &final_env {
        Envelope::Final { ref_id, .. } => ref_id.clone(),
        _ => panic!("expected Final"),
    };
    assert_eq!(event_ref, final_ref);
    assert_eq!(event_ref, run_id);
}

#[test]
fn fatal_ref_id_correlation() {
    let fatal = make_fatal(Some("run-abc"), "err");
    match &fatal {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-abc")),
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 11. Builder pattern
// ===========================================================================

#[test]
fn builder_hello_minimal() {
    let env = EnvelopeBuilder::hello().backend("my-sc").build().unwrap();
    match &env {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "my-sc"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_all_fields() {
    let env = EnvelopeBuilder::hello()
        .backend("sc")
        .version("2.0")
        .adapter_version("1.5")
        .mode(ExecutionMode::Passthrough)
        .capabilities(test_capabilities())
        .build()
        .unwrap();
    match &env {
        Envelope::Hello {
            backend,
            mode,
            capabilities,
            contract_version,
        } => {
            assert_eq!(backend.id, "sc");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.5"));
            assert_eq!(*mode, ExecutionMode::Passthrough);
            assert!(!capabilities.is_empty());
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_missing_backend_errors() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("backend")
    );
}

#[test]
fn builder_run() {
    let env = EnvelopeBuilder::run(test_work_order())
        .ref_id("custom-id")
        .build()
        .unwrap();
    match &env {
        Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_run_default_id_from_work_order() {
    let wo = test_work_order();
    let expected_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match &env {
        Envelope::Run { id, .. } => assert_eq!(id, &expected_id),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_event_requires_ref_id() {
    let err = EnvelopeBuilder::event(test_event()).build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("ref_id")
    );
}

#[test]
fn builder_event_with_ref_id() {
    let env = EnvelopeBuilder::event(test_event())
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Event { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn builder_final_requires_ref_id() {
    let err = EnvelopeBuilder::final_receipt(test_receipt(Uuid::nil()))
        .build()
        .unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("ref_id")
    );
}

#[test]
fn builder_final_with_ref_id() {
    let env = EnvelopeBuilder::final_receipt(test_receipt(Uuid::nil()))
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Final { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn builder_fatal_no_ref_id() {
    let env = EnvelopeBuilder::fatal("crash").build().unwrap();
    match &env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn builder_fatal_with_ref_id() {
    let env = EnvelopeBuilder::fatal("crash")
        .ref_id("run-1")
        .build()
        .unwrap();
    match &env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-1")),
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 12. Version parsing and negotiation
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
    assert_eq!(parse_version("abp/vabc.def"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn incompatible_invalid_strings() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

#[test]
fn protocol_version_parse_and_display() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn protocol_version_current() {
    let v = ProtocolVersion::current();
    assert_eq!(v.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_compatibility() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v01.is_compatible(&v02));
    assert!(!v02.is_compatible(&v01)); // v01.minor < v02.minor
}

#[test]
fn protocol_version_parse_errors() {
    assert!(ProtocolVersion::parse("invalid").is_err());
    assert!(ProtocolVersion::parse("abp/vabc.1").is_err());
    assert!(ProtocolVersion::parse("abp/v1.abc").is_err());
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.2").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.1").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.3").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.4").unwrap()));
}

#[test]
fn negotiate_version_same() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn negotiate_version_picks_min() {
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v0.3").unwrap();
    let result = negotiate_version(&v1, &v2).unwrap();
    assert_eq!(result, v1);
}

#[test]
fn negotiate_version_incompatible_major() {
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&v1, &v2).is_err());
}

// ===========================================================================
// 13. ProtocolError
// ===========================================================================

#[test]
fn protocol_error_json_display() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_violation_has_code() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_error_json_has_no_code() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    assert!(err.error_code().is_none());
}

// ===========================================================================
// 14. Routing
// ===========================================================================

#[test]
fn router_matches_by_type() {
    use abp_protocol::router::{MessageRoute, MessageRouter};

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "hello".into(),
        destination: "handshake_handler".into(),
        priority: 1,
    });
    let hello = make_hello();
    let matched = router.route(&hello).unwrap();
    assert_eq!(matched.destination, "handshake_handler");
}

#[test]
fn router_matches_by_ref_id_prefix() {
    use abp_protocol::router::{MessageRoute, MessageRouter};

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "run-".into(),
        destination: "run_handler".into(),
        priority: 1,
    });
    let event = make_event("run-123");
    let matched = router.route(&event).unwrap();
    assert_eq!(matched.destination, "run_handler");
}

#[test]
fn router_priority_order() {
    use abp_protocol::router::{MessageRoute, MessageRouter};

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "event".into(),
        destination: "low".into(),
        priority: 1,
    });
    router.add_route(MessageRoute {
        pattern: "event".into(),
        destination: "high".into(),
        priority: 10,
    });
    let event = make_event("r1");
    let matched = router.route(&event).unwrap();
    assert_eq!(matched.destination, "high");
}

#[test]
fn router_no_match_returns_none() {
    use abp_protocol::router::MessageRouter;

    let router = MessageRouter::new();
    assert!(router.route(&make_hello()).is_none());
}

#[test]
fn router_route_all() {
    use abp_protocol::router::{MessageRoute, MessageRouter};

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "event".into(),
        destination: "ev".into(),
        priority: 1,
    });
    let envs = vec![make_hello(), make_event("r1"), make_event("r2")];
    let matches = router.route_all(&envs);
    assert_eq!(matches.len(), 2);
}

#[test]
fn router_remove_route() {
    use abp_protocol::router::{MessageRoute, MessageRouter};

    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "hello".into(),
        destination: "h".into(),
        priority: 1,
    });
    assert_eq!(router.route_count(), 1);
    router.remove_route("h");
    assert_eq!(router.route_count(), 0);
}

// ===========================================================================
// 15. RouteTable
// ===========================================================================

#[test]
fn route_table_insert_and_lookup() {
    use abp_protocol::router::RouteTable;

    let mut table = RouteTable::new();
    table.insert("hello", "handler_a");
    assert_eq!(table.lookup("hello"), Some("handler_a"));
    assert_eq!(table.lookup("run"), None);
}

#[test]
fn route_table_entries() {
    use abp_protocol::router::RouteTable;

    let mut table = RouteTable::new();
    table.insert("event", "ev_handler");
    table.insert("fatal", "err_handler");
    assert_eq!(table.entries().len(), 2);
}

// ===========================================================================
// 16. Envelope clone and debug
// ===========================================================================

#[test]
fn envelope_clone() {
    let orig = make_hello();
    let cloned = orig.clone();
    let orig_json = JsonlCodec::encode(&orig).unwrap();
    let cloned_json = JsonlCodec::encode(&cloned).unwrap();
    assert_eq!(orig_json, cloned_json);
}

#[test]
fn envelope_debug_format() {
    let env = make_hello();
    let debug = format!("{env:?}");
    assert!(debug.contains("Hello"));
}

// ===========================================================================
// 17. Discriminator is "t" not "type"
// ===========================================================================

#[test]
fn discriminator_field_is_t_hello() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("t").is_some(), "missing 't' field");
    assert!(v.get("type").is_none(), "should not have 'type' field");
    assert_eq!(v["t"], "hello");
}

#[test]
fn discriminator_field_is_t_run() {
    let json = JsonlCodec::encode(&make_run("r1")).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["t"], "run");
}

#[test]
fn discriminator_field_is_t_event() {
    let json = JsonlCodec::encode(&make_event("r1")).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn discriminator_field_is_t_final() {
    let json = JsonlCodec::encode(&make_final("r1")).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn discriminator_field_is_t_fatal() {
    let json = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["t"], "fatal");
}

// ===========================================================================
// 18. Event with ext field
// ===========================================================================

#[test]
fn event_with_ext_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor_key".to_string(), serde_json::json!("val"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext),
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert_eq!(ext["vendor_key"], serde_json::json!("val"));
        }
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// 19. Receipt with various outcomes
// ===========================================================================

#[test]
fn receipt_outcome_partial_roundtrip() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Partial;
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_outcome_failed_roundtrip() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Failed;
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        _ => panic!("expected Final"),
    }
}

// ===========================================================================
// 20. Capability manifest in hello
// ===========================================================================

#[test]
fn hello_with_multiple_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    let env = Envelope::hello(test_identity(), caps.clone());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_empty_capabilities_roundtrip() {
    let env = Envelope::hello(test_identity(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// 21. Batch processing
// ===========================================================================

#[test]
fn batch_processor_processes_valid_envelopes() {
    use abp_protocol::batch::{BatchItemStatus, BatchProcessor, BatchRequest};

    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "batch-1".into(),
        envelopes: vec![make_hello(), make_fatal(None, "err")],
        created_at: Utc::now().to_rfc3339(),
    };
    let response = processor.process(request);
    assert_eq!(response.request_id, "batch-1");
    assert_eq!(response.results.len(), 2);
    assert!(
        response
            .results
            .iter()
            .all(|r| r.status == BatchItemStatus::Success)
    );
}

#[test]
fn batch_validate_empty_batch() {
    use abp_protocol::batch::{BatchProcessor, BatchRequest, BatchValidationError};

    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "empty".into(),
        envelopes: vec![],
        created_at: Utc::now().to_rfc3339(),
    };
    let errors = processor.validate_batch(&request);
    assert!(errors.contains(&BatchValidationError::EmptyBatch));
}

// ===========================================================================
// 22. Work order with non-trivial fields roundtrip
// ===========================================================================

#[test]
fn work_order_with_context_roundtrip() {
    let mut wo = test_work_order();
    wo.context.files.push("/src/main.rs".into());
    wo.config.model = Some("gpt-4".into());
    wo.lane = ExecutionLane::WorkspaceFirst;
    wo.workspace.mode = WorkspaceMode::Staged;
    wo.workspace.include.push("**/*.rs".into());
    wo.workspace.exclude.push("target/**".into());

    let env = Envelope::Run {
        id: "r-ctx".into(),
        work_order: wo.clone(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files, vec!["/src/main.rs"]);
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
            assert!(matches!(work_order.lane, ExecutionLane::WorkspaceFirst));
            assert!(matches!(work_order.workspace.mode, WorkspaceMode::Staged));
            assert_eq!(work_order.workspace.include, vec!["**/*.rs"]);
            assert_eq!(work_order.workspace.exclude, vec!["target/**"]);
        }
        _ => panic!("expected Run"),
    }
}

// ===========================================================================
// 23. Serde rename_all snake_case for all tags
// ===========================================================================

#[test]
fn all_envelope_tags_are_snake_case() {
    let cases: Vec<(Envelope, &str)> = vec![
        (make_hello(), "hello"),
        (make_run("r1"), "run"),
        (make_event("r1"), "event"),
        (make_final("r1"), "final"),
        (make_fatal(None, "err"), "fatal"),
    ];
    for (env, expected_tag) in cases {
        let json = JsonlCodec::encode(&env).unwrap();
        let v: Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), expected_tag);
    }
}

// ===========================================================================
// 24. Large sequence with many events validates correctly
// ===========================================================================

#[test]
fn large_sequence_validates() {
    let mut seq = vec![make_hello(), make_run("r1")];
    for _ in 0..100 {
        seq.push(make_event("r1"));
    }
    seq.push(make_final("r1"));

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty());
}

// ===========================================================================
// 25. Execution mode roundtrip
// ===========================================================================

#[test]
fn execution_mode_mapped_default() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_in_hello() {
    let env = Envelope::hello_with_mode(
        test_identity(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// 26. ValidationError / ValidationWarning Display
// ===========================================================================

#[test]
fn validation_error_display() {
    let e = ValidationError::MissingField {
        field: "foo".into(),
    };
    assert!(e.to_string().contains("foo"));

    let e = ValidationError::InvalidVersion {
        version: "bad".into(),
    };
    assert!(e.to_string().contains("bad"));

    let e = ValidationError::EmptyField {
        field: "bar".into(),
    };
    assert!(e.to_string().contains("bar"));

    let e = ValidationError::InvalidValue {
        field: "f".into(),
        value: "v".into(),
        expected: "e".into(),
    };
    assert!(e.to_string().contains("f"));
}

#[test]
fn validation_warning_display() {
    let w = ValidationWarning::DeprecatedField {
        field: "old".into(),
    };
    assert!(w.to_string().contains("old"));

    let w = ValidationWarning::LargePayload {
        size: 100,
        max_recommended: 50,
    };
    assert!(w.to_string().contains("100"));

    let w = ValidationWarning::MissingOptionalField {
        field: "opt".into(),
    };
    assert!(w.to_string().contains("opt"));
}

#[test]
fn sequence_error_display() {
    assert!(!SequenceError::MissingHello.to_string().is_empty());
    assert!(!SequenceError::MissingTerminal.to_string().is_empty());
    assert!(
        !SequenceError::HelloNotFirst { position: 2 }
            .to_string()
            .is_empty()
    );
    assert!(!SequenceError::MultipleTerminals.to_string().is_empty());
    assert!(
        !SequenceError::RefIdMismatch {
            expected: "a".into(),
            found: "b".into(),
        }
        .to_string()
        .is_empty()
    );
    assert!(!SequenceError::OutOfOrderEvents.to_string().is_empty());
}

// ===========================================================================
// 27. ProtocolError Display
// ===========================================================================

#[test]
fn protocol_error_violation_display() {
    let e = ProtocolError::Violation("bad state".into());
    assert!(e.to_string().contains("bad state"));
}

#[test]
fn protocol_error_unexpected_message_display() {
    let e = ProtocolError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert!(e.to_string().contains("run"));
    assert!(e.to_string().contains("hello"));
}

// ===========================================================================
// 28. Contract version constant
// ===========================================================================

#[test]
fn contract_version_is_parseable() {
    let (major, minor) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn contract_version_format() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
}

// ===========================================================================
// 29. RawFrame (sidecar-kit) re-export
// ===========================================================================

#[test]
fn raw_frame_hello_roundtrip() {
    use abp_protocol::RawFrame;

    let frame = RawFrame::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: serde_json::json!({"id": "test"}),
        capabilities: serde_json::json!({}),
        mode: serde_json::json!("mapped"),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    let decoded: RawFrame = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, RawFrame::Hello { .. }));
}

#[test]
fn raw_frame_fatal_roundtrip() {
    use abp_protocol::RawFrame;

    let frame = RawFrame::Fatal {
        ref_id: Some("r1".into()),
        error: "crash".into(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    let decoded: RawFrame = serde_json::from_str(&json).unwrap();
    match decoded {
        RawFrame::Fatal { ref_id, error } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn raw_frame_event_roundtrip() {
    use abp_protocol::RawFrame;

    let frame = RawFrame::Event {
        ref_id: "r1".into(),
        event: serde_json::json!({"kind": "test"}),
    };
    let json = serde_json::to_string(&frame).unwrap();
    let decoded: RawFrame = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, RawFrame::Event { .. }));
}

#[test]
fn raw_frame_ping_pong() {
    use abp_protocol::RawFrame;

    let ping = RawFrame::Ping { seq: 42 };
    let json = serde_json::to_string(&ping).unwrap();
    assert!(json.contains(r#""t":"ping""#));
    let decoded: RawFrame = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, RawFrame::Ping { seq: 42 }));

    let pong = RawFrame::Pong { seq: 42 };
    let json = serde_json::to_string(&pong).unwrap();
    assert!(json.contains(r#""t":"pong""#));
}

#[test]
fn raw_frame_cancel_roundtrip() {
    use abp_protocol::RawFrame;

    let frame = RawFrame::Cancel {
        ref_id: "r1".into(),
        reason: Some("timeout".into()),
    };
    let json = serde_json::to_string(&frame).unwrap();
    let decoded: RawFrame = serde_json::from_str(&json).unwrap();
    match decoded {
        RawFrame::Cancel { ref_id, reason } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(reason, Some("timeout".into()));
        }
        _ => panic!("expected Cancel"),
    }
}

// ===========================================================================
// 30. VersionRange compatibility
// ===========================================================================

#[test]
fn version_range_is_compatible() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.is_compatible(&ProtocolVersion::parse("abp/v0.2").unwrap()));
    assert!(!range.is_compatible(&ProtocolVersion::parse("abp/v1.0").unwrap()));
}

#[test]
fn version_range_not_compatible_different_major() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    assert!(!range.is_compatible(&ProtocolVersion::parse("abp/v1.2").unwrap()));
}

// ===========================================================================
// 31. Envelope error_code on non-fatal returns None
// ===========================================================================

#[test]
fn error_code_none_for_run() {
    assert!(make_run("r1").error_code().is_none());
}

#[test]
fn error_code_none_for_event() {
    assert!(make_event("r1").error_code().is_none());
}

#[test]
fn error_code_none_for_final() {
    assert!(make_final("r1").error_code().is_none());
}

#[test]
fn error_code_some_for_fatal_with_code() {
    let env = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    assert!(env.error_code().is_some());
}

#[test]
fn error_code_none_for_fatal_without_code() {
    let env = make_fatal(None, "err");
    assert!(env.error_code().is_none());
}

// ===========================================================================
// 30. StreamParser – partial lines, buffering, edge cases
// ===========================================================================

#[test]
fn stream_parser_partial_line_buffering() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "streamed")).unwrap();
    let (a, b) = line.as_bytes().split_at(10);
    let r1 = parser.feed(a);
    assert!(r1.is_empty(), "partial line should not produce results");
    let r2 = parser.feed(b);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_chunk() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "a")).unwrap();
    let two_lines = format!("{}{}", line, line);
    let results = parser.feed(two_lines.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_blank_lines_skipped() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "x")).unwrap();
    let input = format!("\n\n{}\n\n", line.trim());
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_finish_unterminated_line() {
    let mut parser = StreamParser::new();
    let json_no_newline = r#"{"t":"fatal","ref_id":null,"error":"unterminated"}"#;
    parser.feed(json_no_newline.as_bytes());
    assert!(!parser.is_empty());
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_buffered_len_and_reset() {
    let mut parser = StreamParser::new();
    assert_eq!(parser.buffered_len(), 0);
    parser.feed(b"partial data");
    assert_eq!(parser.buffered_len(), 12);
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_max_line_len_exceeded() {
    let mut parser = StreamParser::with_max_line_len(10);
    let long_line = format!("{}\n", "x".repeat(20));
    let results = parser.feed(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_byte_by_byte_delivery() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "byte")).unwrap();
    let mut total = vec![];
    for byte in line.as_bytes() {
        total.extend(parser.feed(&[*byte]));
    }
    assert_eq!(total.len(), 1);
    assert!(total[0].is_ok());
}

#[test]
fn stream_parser_invalid_utf8() {
    let mut parser = StreamParser::new();
    let bad = [0xFF, 0xFE, b'\n'];
    let results = parser.feed(&bad);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_default_trait() {
    let p = StreamParser::default();
    assert!(p.is_empty());
}

#[test]
fn stream_parser_full_lifecycle_parsing() {
    let mut parser = StreamParser::new();
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let run = JsonlCodec::encode(&make_run("r1")).unwrap();
    let event = JsonlCodec::encode(&make_event("r1")).unwrap();
    let fin = JsonlCodec::encode(&make_final("r1")).unwrap();

    let all = format!("{}{}{}{}", hello, run, event, fin);
    let results = parser.feed(all.as_bytes());
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|r| r.is_ok()));
}

// ===========================================================================
// 31. Protocol state machine (sidecar-kit ProtocolState)
// ===========================================================================

#[test]
fn state_machine_happy_path() {
    let mut state = ProtocolState::new();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);

    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);

    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Streaming);
    assert_eq!(state.run_id(), Some("r1"));

    state
        .advance(&Frame::Event {
            ref_id: "r1".into(),
            event: json!({"type": "assistant_delta", "text": "hi"}),
        })
        .unwrap();
    assert_eq!(state.events_seen(), 1);

    state
        .advance(&Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        })
        .unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
    assert!(state.is_terminal());
}

#[test]
fn state_machine_fatal_during_streaming() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    state
        .advance(&Frame::Fatal {
            ref_id: Some("r1".into()),
            error: "crash".into(),
        })
        .unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
}

#[test]
fn state_machine_event_before_hello_faults() {
    let mut state = ProtocolState::new();
    let result = state.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
}

#[test]
fn state_machine_run_before_hello_faults() {
    let mut state = ProtocolState::new();
    let result = state.advance(&Frame::Run {
        id: "r1".into(),
        work_order: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
}

#[test]
fn state_machine_double_hello_faults() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    let result = state.advance(&Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "t2"}),
        capabilities: json!({}),
        mode: Value::Null,
    });
    assert!(result.is_err());
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
}

#[test]
fn state_machine_event_after_final_faults() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    state
        .advance(&Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        })
        .unwrap();
    let result = state.advance(&Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    });
    assert!(result.is_err());
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
}

#[test]
fn state_machine_ref_id_mismatch_rejects() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    let result = state.advance(&Frame::Event {
        ref_id: "WRONG".into(),
        event: json!({}),
    });
    assert!(result.is_err());
}

#[test]
fn state_machine_reset_clears_state() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state.reset();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
    assert!(state.run_id().is_none());
    assert_eq!(state.events_seen(), 0);
}

#[test]
fn state_machine_faulted_rejects_all_frames() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Event {
            ref_id: "x".into(),
            event: json!({}),
        })
        .unwrap_err();
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
    assert!(state.fault_reason().is_some());
    let result = state.advance(&Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "t"}),
        capabilities: json!({}),
        mode: Value::Null,
    });
    assert!(result.is_err());
}

#[test]
fn state_machine_ping_pong_during_streaming() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    state.advance(&Frame::Ping { seq: 1 }).unwrap();
    state.advance(&Frame::Pong { seq: 1 }).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Streaming);
}

#[test]
fn state_machine_fatal_while_awaiting_run() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Fatal {
            ref_id: None,
            error: "startup failure".into(),
        })
        .unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
}

#[test]
fn state_machine_multiple_events_counted() {
    let mut state = ProtocolState::new();
    state
        .advance(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "t"}),
            capabilities: json!({}),
            mode: Value::Null,
        })
        .unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
    for _ in 0..5 {
        state
            .advance(&Frame::Event {
                ref_id: "r1".into(),
                event: json!({}),
            })
            .unwrap();
    }
    assert_eq!(state.events_seen(), 5);
}

// ===========================================================================
// 32. Frame validation (sidecar-kit validate_frame)
// ===========================================================================

#[test]
fn validate_frame_valid_hello_ok() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(result.valid, "issues: {:?}", result.issues);
}

#[test]
fn validate_frame_empty_contract_version_fails() {
    let frame = Frame::Hello {
        contract_version: "".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_bad_version_prefix() {
    let frame = Frame::Hello {
        contract_version: "xyz/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_empty_run_id_fails() {
    let frame = Frame::Run {
        id: "".into(),
        work_order: json!({}),
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_empty_event_ref_id_fails() {
    let frame = Frame::Event {
        ref_id: "".into(),
        event: json!({}),
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_empty_fatal_error_fails() {
    let frame = Frame::Fatal {
        ref_id: None,
        error: "".into(),
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_size_exceeded_fails() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"big": "x".repeat(1000)}),
    };
    let result = validate_frame(&frame, 50);
    assert!(!result.valid);
}

#[test]
fn validate_frame_empty_cancel_ref_id_fails() {
    let frame = Frame::Cancel {
        ref_id: "".into(),
        reason: None,
    };
    let result = validate_frame(&frame, 1_000_000);
    assert!(!result.valid);
}

#[test]
fn validate_frame_ping_always_valid() {
    let result = validate_frame(&Frame::Ping { seq: 0 }, 1_000_000);
    assert!(result.valid);
}

// ===========================================================================
// 33. FrameWriter / FrameReader roundtrips
// ===========================================================================

#[test]
fn frame_writer_reader_full_roundtrip() {
    let frames = vec![
        Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: Value::Null,
        },
        Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        },
        Frame::Event {
            ref_id: "r1".into(),
            event: json!({"type": "run_started"}),
        },
        Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        },
    ];
    let mut buf = Vec::new();
    let count = write_frames(&mut buf, &frames).unwrap();
    assert_eq!(count, 4);
    let reader = buf_reader_from_bytes(&buf);
    let read_back = read_all_frames(reader).unwrap();
    assert_eq!(read_back.len(), 4);
}

#[test]
fn frame_writer_tracks_count() {
    let mut buf = Vec::new();
    let mut writer = FrameWriter::new(&mut buf);
    writer.write_frame(&Frame::Ping { seq: 1 }).unwrap();
    writer.write_frame(&Frame::Pong { seq: 1 }).unwrap();
    assert_eq!(writer.frames_written(), 2);
}

#[test]
fn frame_reader_skips_blank_lines() {
    let input = b"\n\n{\"t\":\"ping\",\"seq\":1}\n\n";
    let reader = buf_reader_from_bytes(input);
    let frames = read_all_frames(reader).unwrap();
    assert_eq!(frames.len(), 1);
}

#[test]
fn frame_reader_max_size_enforcement() {
    let mut big = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"".to_string();
    big.push_str(&"x".repeat(200));
    big.push_str("\"}\n");
    let reader = buf_reader_from_bytes(big.as_bytes());
    let mut fr = FrameReader::with_max_size(reader, 50);
    assert!(fr.read_frame().is_err());
}

// ===========================================================================
// 34. sidecar-kit builder functions
// ===========================================================================

#[test]
fn sidecar_kit_hello_frame_builder() {
    let frame = sidecar_kit::hello_frame("my-backend");
    match frame {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "my-backend");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn sidecar_kit_event_frame_builder() {
    let frame = sidecar_kit::event_frame("r1", sidecar_kit::event_text_delta("token"));
    match &frame {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(event["type"], "assistant_delta");
            assert_eq!(event["text"], "token");
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn sidecar_kit_fatal_frame_builder() {
    let frame = sidecar_kit::fatal_frame(Some("r1"), "error msg");
    match frame {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("r1"));
            assert_eq!(error, "error msg");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn sidecar_kit_event_text_message() {
    let v = sidecar_kit::event_text_message("complete text");
    assert_eq!(v["type"], "assistant_message");
    assert_eq!(v["text"], "complete text");
}

#[test]
fn sidecar_kit_event_tool_call() {
    let v = sidecar_kit::event_tool_call("read_file", Some("tc1"), json!({"path": "f.txt"}));
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "read_file");
    assert_eq!(v["tool_use_id"], "tc1");
}

#[test]
fn sidecar_kit_event_tool_result() {
    let v = sidecar_kit::event_tool_result("read_file", Some("tc1"), json!("data"), false);
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["is_error"], false);
}

#[test]
fn sidecar_kit_event_file_changed() {
    let v = sidecar_kit::event_file_changed("src/lib.rs", "added function");
    assert_eq!(v["type"], "file_changed");
    assert_eq!(v["path"], "src/lib.rs");
}

#[test]
fn sidecar_kit_event_command_executed() {
    let v = sidecar_kit::event_command_executed("cargo test", Some(0), Some("ok"));
    assert_eq!(v["type"], "command_executed");
    assert_eq!(v["exit_code"], 0);
}

#[test]
fn sidecar_kit_event_warning_and_error() {
    let w = sidecar_kit::event_warning("deprecation");
    assert_eq!(w["type"], "warning");
    let e = sidecar_kit::event_error("critical");
    assert_eq!(e["type"], "error");
}

#[test]
fn sidecar_kit_event_run_started_completed() {
    let s = sidecar_kit::event_run_started("beginning");
    assert_eq!(s["type"], "run_started");
    let c = sidecar_kit::event_run_completed("finished");
    assert_eq!(c["type"], "run_completed");
}

#[test]
fn sidecar_kit_receipt_builder() {
    let receipt = sidecar_kit::ReceiptBuilder::new("r1", "test-backend")
        .event(sidecar_kit::event_text_delta("hello"))
        .artifact("diff", "output.patch")
        .input_tokens(100)
        .output_tokens(50)
        .build();
    assert_eq!(receipt["meta"]["run_id"], "r1");
    assert_eq!(receipt["backend"]["id"], "test-backend");
    assert_eq!(receipt["outcome"], "complete");
    assert_eq!(receipt["trace"].as_array().unwrap().len(), 1);
    assert_eq!(receipt["artifacts"].as_array().unwrap().len(), 1);
}

#[test]
fn sidecar_kit_receipt_builder_failed() {
    let receipt = sidecar_kit::ReceiptBuilder::new("r1", "t").failed().build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn sidecar_kit_receipt_builder_partial() {
    let receipt = sidecar_kit::ReceiptBuilder::new("r1", "t")
        .partial()
        .build();
    assert_eq!(receipt["outcome"], "partial");
}

// ===========================================================================
// 35. Frame typed extraction
// ===========================================================================

#[test]
fn frame_try_event_typed_extraction() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"text": "hello"}),
    };
    #[derive(serde::Deserialize)]
    struct Evt {
        text: String,
    }
    let (rid, evt): (String, Evt) = frame.try_event().unwrap();
    assert_eq!(rid, "r1");
    assert_eq!(evt.text, "hello");
}

#[test]
fn frame_try_event_on_wrong_type_fails() {
    let frame = Frame::Fatal {
        ref_id: None,
        error: "e".into(),
    };
    let result: Result<(String, Value), _> = frame.try_event();
    assert!(result.is_err());
}

#[test]
fn frame_try_final_typed_extraction() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({"outcome": "complete"}),
    };
    #[derive(serde::Deserialize)]
    struct Rcpt {
        outcome: String,
    }
    let (rid, rcpt): (String, Rcpt) = frame.try_final().unwrap();
    assert_eq!(rid, "r1");
    assert_eq!(rcpt.outcome, "complete");
}

#[test]
fn frame_try_final_on_wrong_type_fails() {
    let frame = Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = frame.try_final();
    assert!(result.is_err());
}

// ===========================================================================
// 36. StreamingCodec batch operations
// ===========================================================================

#[test]
fn streaming_codec_encode_decode_batch() {
    let envs = vec![make_fatal(None, "a"), make_fatal(None, "b")];
    let batch = StreamingCodec::encode_batch(&envs);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn streaming_codec_line_count() {
    let input = "line1\n\nline2\nline3\n\n";
    assert_eq!(StreamingCodec::line_count(input), 3);
}

#[test]
fn streaming_codec_validate_jsonl_mixed() {
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok\"}\nnot json\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"ok2\"}\n";
    let errors = StreamingCodec::validate_jsonl(input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2);
}

// ===========================================================================
// 37. Batch processor
// ===========================================================================

#[test]
fn batch_processor_processes_all_items() {
    let processor = BatchProcessor::new();
    let req = BatchRequest {
        id: "b1".into(),
        envelopes: vec![make_fatal(None, "a"), make_fatal(None, "b")],
        created_at: Utc::now().to_rfc3339(),
    };
    let resp = processor.process(req);
    assert_eq!(resp.request_id, "b1");
    assert_eq!(resp.results.len(), 2);
}

#[test]
fn batch_validate_empty_batch_error() {
    let processor = BatchProcessor::new();
    let req = BatchRequest {
        id: "b1".into(),
        envelopes: vec![],
        created_at: Utc::now().to_rfc3339(),
    };
    let errors = processor.validate_batch(&req);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, BatchValidationError::EmptyBatch))
    );
}

// ===========================================================================
// 38. Compression roundtrips on envelopes
// ===========================================================================

#[test]
fn gzip_compress_envelope_roundtrip() {
    let env = make_hello();
    let json_bytes = JsonlCodec::encode(&env).unwrap();
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let compressed = c.compress(json_bytes.as_bytes()).unwrap();
    let decompressed = c.decompress(&compressed).unwrap();
    assert_eq!(decompressed, json_bytes.as_bytes());
}

#[test]
fn zstd_compress_envelope_roundtrip() {
    let env = make_hello();
    let json_bytes = JsonlCodec::encode(&env).unwrap();
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let compressed = c.compress(json_bytes.as_bytes()).unwrap();
    let decompressed = c.decompress(&compressed).unwrap();
    assert_eq!(decompressed, json_bytes.as_bytes());
}

#[test]
fn compression_stats_tracking() {
    let mut stats = CompressionStats::new();
    stats.record(1000, 200);
    assert_eq!(stats.bytes_saved(), 800);
    assert!((stats.compression_ratio() - 0.2).abs() < f64::EPSILON);
}

#[test]
fn compress_message_wrapper() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"hello compressed message roundtrip";
    let msg = c.compress_message(data).unwrap();
    assert_eq!(msg.algorithm, CompressionAlgorithm::Gzip);
    assert_eq!(msg.original_size, data.len());
    let decompressed = c.decompress_message(&msg).unwrap();
    assert_eq!(decompressed, data);
}

// ===========================================================================
// 39. Version negotiation edge cases
// ===========================================================================

#[test]
fn version_error_invalid_format() {
    assert!(matches!(
        ProtocolVersion::parse("invalid"),
        Err(VersionError::InvalidFormat)
    ));
}

#[test]
fn version_error_invalid_major() {
    assert!(matches!(
        ProtocolVersion::parse("abp/vx.1"),
        Err(VersionError::InvalidMajor)
    ));
}

#[test]
fn version_error_invalid_minor() {
    assert!(matches!(
        ProtocolVersion::parse("abp/v0.y"),
        Err(VersionError::InvalidMinor)
    ));
}

#[test]
fn negotiate_version_picks_minimum() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn negotiate_version_incompatible_major_versions() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(matches!(
        negotiate_version(&v0, &v1),
        Err(VersionError::Incompatible { .. })
    ));
}

// ===========================================================================
// 40. Frame encode/decode roundtrips (json_to_frame / frame_to_json)
// ===========================================================================

#[test]
fn frame_to_json_hello_roundtrip() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "test"}),
        capabilities: json!({}),
        mode: Value::Null,
    };
    let j = frame_to_json(&frame).unwrap();
    let decoded = json_to_frame(&j).unwrap();
    assert!(matches!(decoded, Frame::Hello { .. }));
}

#[test]
fn frame_to_json_cancel_roundtrip() {
    let frame = Frame::Cancel {
        ref_id: "r1".into(),
        reason: Some("timeout".into()),
    };
    let j = frame_to_json(&frame).unwrap();
    let decoded = json_to_frame(&j).unwrap();
    assert!(matches!(decoded, Frame::Cancel { .. }));
}

#[test]
fn frame_to_json_ping_pong_roundtrip() {
    for frame in [Frame::Ping { seq: 99 }, Frame::Pong { seq: 99 }] {
        let j = frame_to_json(&frame).unwrap();
        let decoded = json_to_frame(&j).unwrap();
        match (&frame, &decoded) {
            (Frame::Ping { seq: a }, Frame::Ping { seq: b }) => assert_eq!(a, b),
            (Frame::Pong { seq: a }, Frame::Pong { seq: b }) => assert_eq!(a, b),
            _ => panic!("mismatch"),
        }
    }
}
