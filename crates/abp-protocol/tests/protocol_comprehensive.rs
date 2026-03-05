#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use std::collections::BTreeMap;
use std::io::{BufReader, Cursor};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Receipt,
    ReceiptBuilder, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::batch::{
    BatchItemStatus, BatchProcessor, BatchRequest, BatchResponse, BatchResult,
    BatchValidationError, MAX_BATCH_SIZE,
};
use abp_protocol::builder::{BuilderError, EnvelopeBuilder};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::compress::{
    CompressError, CompressedMessage, CompressionAlgorithm, CompressionStats, MessageCompressor,
};
use abp_protocol::router::{MessageRoute, MessageRouter, RouteTable};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===== Helpers =====

fn make_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn make_caps() -> CapabilityManifest {
    CapabilityManifest::new()
}

fn make_hello() -> Envelope {
    Envelope::hello(make_backend(), make_caps())
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("hello world").build()
}

fn make_receipt() -> Receipt {
    ReceiptBuilder::new("test-backend").build()
}

fn make_agent_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    }
}

fn make_run() -> Envelope {
    let wo = make_work_order();
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: make_agent_event(),
    }
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: make_receipt(),
    }
}

fn make_fatal(ref_id: Option<&str>) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: "something went wrong".into(),
        error_code: None,
    }
}

// =========================================================================
// 1. Envelope variant construction
// =========================================================================

#[test]
fn hello_envelope_construction() {
    let env = make_hello();
    match &env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert!(backend.adapter_version.is_none());
            assert!(capabilities.is_empty());
            assert_eq!(*mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_mode_passthrough() {
    let env = Envelope::hello_with_mode(make_backend(), make_caps(), ExecutionMode::Passthrough);
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_mode_mapped() {
    let env = Envelope::hello_with_mode(make_backend(), make_caps(), ExecutionMode::Mapped);
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn run_envelope_construction() {
    let wo = make_work_order();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    match &env {
        Envelope::Run {
            id: run_id,
            work_order,
        } => {
            assert_eq!(run_id, &id);
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn event_envelope_construction() {
    let env = make_event("run-123");
    match &env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-123");
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn final_envelope_construction() {
    let env = make_final("run-123");
    match &env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-123");
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_envelope_with_ref_id() {
    let env = make_fatal(Some("run-123"));
    match &env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-123"));
            assert_eq!(error, "something went wrong");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_without_ref_id() {
    let env = make_fatal(None);
    match &env {
        Envelope::Fatal { ref_id, .. } => {
            assert!(ref_id.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "auth failed",
        abp_error::ErrorCode::BackendAuthFailed,
    );
    match &env {
        Envelope::Fatal {
            error_code,
            error,
            ref_id,
        } => {
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendAuthFailed));
            assert_eq!(error, "auth failed");
            assert_eq!(ref_id.as_deref(), Some("run-1"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_from_abp_error() {
    let abp_err =
        abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "request timed out");
    let env = Envelope::fatal_from_abp_error(Some("r1".into()), &abp_err);
    match &env {
        Envelope::Fatal {
            error,
            error_code,
            ref_id,
        } => {
            assert_eq!(error, "request timed out");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
            assert_eq!(ref_id.as_deref(), Some("r1"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_error_code_on_fatal() {
    let env = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::Internal);
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::Internal));
}

#[test]
fn envelope_error_code_on_non_fatal() {
    let env = make_hello();
    assert_eq!(env.error_code(), None);

    let env = make_run();
    assert_eq!(env.error_code(), None);
}

// =========================================================================
// 2. Serde tag discriminator: "t" not "type"
// =========================================================================

#[test]
fn hello_json_uses_t_discriminator() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains(r#""t":"hello""#), "JSON: {json}");
    assert!(!json.contains(r#""type":"hello""#), "JSON: {json}");
}

#[test]
fn run_json_uses_t_discriminator() {
    let json = JsonlCodec::encode(&make_run()).unwrap();
    assert!(json.contains(r#""t":"run""#), "JSON: {json}");
}

#[test]
fn event_json_uses_t_discriminator() {
    let json = JsonlCodec::encode(&make_event("r1")).unwrap();
    assert!(json.contains(r#""t":"event""#), "JSON: {json}");
}

#[test]
fn final_json_uses_t_discriminator() {
    let json = JsonlCodec::encode(&make_final("r1")).unwrap();
    assert!(json.contains(r#""t":"final""#), "JSON: {json}");
}

#[test]
fn fatal_json_uses_t_discriminator() {
    let json = JsonlCodec::encode(&make_fatal(None)).unwrap();
    assert!(json.contains(r#""t":"fatal""#), "JSON: {json}");
}

#[test]
fn serde_rename_all_snake_case() {
    // All envelope variants should be snake_case in JSON
    let variants = vec![
        (make_hello(), "hello"),
        (make_run(), "run"),
        (make_event("r"), "event"),
        (make_final("r"), "final"),
        (make_fatal(None), "fatal"),
    ];
    for (env, expected_t) in variants {
        let json = JsonlCodec::encode(&env).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), expected_t);
    }
}

// =========================================================================
// 3. JsonlCodec encode/decode roundtrips
// =========================================================================

#[test]
fn hello_roundtrip() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn run_roundtrip() {
    let env = make_run();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn event_roundtrip() {
    let env = make_event("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn final_roundtrip() {
    let env = make_final("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_roundtrip() {
    let env = make_fatal(Some("run-42"));
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "something went wrong");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_code_roundtrip() {
    let env =
        Envelope::fatal_with_code(Some("r".into()), "err", abp_error::ErrorCode::PolicyDenied);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::PolicyDenied)
    );
}

#[test]
fn encode_ends_with_newline() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn encode_single_line() {
    let env = make_fatal(None);
    let json = JsonlCodec::encode(&env).unwrap();
    // Should be exactly one line (plus trailing newline)
    let lines: Vec<&str> = json.trim().lines().collect();
    assert_eq!(lines.len(), 1);
}

// =========================================================================
// 4. JsonlCodec decode_stream
// =========================================================================

#[test]
fn decode_stream_multiple_lines() {
    let e1 = JsonlCodec::encode(&make_fatal(None)).unwrap();
    let e2 = JsonlCodec::encode(&make_fatal(Some("r"))).unwrap();
    let input = format!("{e1}{e2}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn decode_stream_skips_blank_lines() {
    let e1 = JsonlCodec::encode(&make_fatal(None)).unwrap();
    let input = format!("\n\n{e1}\n\n");
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

// =========================================================================
// 5. JsonlCodec encode_to_writer / encode_many_to_writer
// =========================================================================

#[test]
fn encode_to_writer_produces_jsonl() {
    let env = make_fatal(None);
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"fatal""#));
}

#[test]
fn encode_many_to_writer_produces_multiple_lines() {
    let envs = vec![make_fatal(None), make_fatal(Some("r"))];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.trim().lines().collect();
    assert_eq!(lines.len(), 2);
}

// =========================================================================
// 6. ProtocolError variants and Display
// =========================================================================

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    let display = format!("{err}");
    assert!(display.contains("invalid JSON"));
}

#[test]
fn protocol_error_violation() {
    let err = ProtocolError::Violation("bad envelope".into());
    let display = format!("{err}");
    assert!(display.contains("protocol violation"));
    assert!(display.contains("bad envelope"));
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let display = format!("{err}");
    assert!(display.contains("unexpected message"));
    assert!(display.contains("hello"));
    assert!(display.contains("run"));
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn protocol_error_io_variant() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
    let err = ProtocolError::Io(io_err);
    let display = format!("{err}");
    assert!(display.contains("I/O error"));
}

#[test]
fn protocol_error_abp_variant() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::Internal, "oops");
    let err = ProtocolError::from(abp_err);
    assert!(matches!(err, ProtocolError::Abp(_)));
    assert_eq!(err.error_code(), Some(abp_error::ErrorCode::Internal));
}

#[test]
fn protocol_error_json_has_no_error_code() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    assert!(err.error_code().is_none());
}

// =========================================================================
// 7. parse_version and is_compatible_version
// =========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v1"), None);
    assert_eq!(parse_version("abp/v1."), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_version_contract_version() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn is_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
}

#[test]
fn is_compatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn is_compatible_invalid_strings() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
    assert!(!is_compatible_version("invalid", "invalid"));
}

// =========================================================================
// 8. Edge cases: malformed JSON, missing fields, extra fields
// =========================================================================

#[test]
fn decode_missing_t_field() {
    let err = JsonlCodec::decode(r#"{"error":"boom"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_unknown_t_value() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_variant","data":123}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_empty_string() {
    let err = JsonlCodec::decode("").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_just_whitespace() {
    let err = JsonlCodec::decode("   ").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_null() {
    let err = JsonlCodec::decode("null").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_array() {
    let err = JsonlCodec::decode("[]").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_number() {
    let err = JsonlCodec::decode("42").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_fatal_extra_fields_ignored() {
    // Extra fields beyond what Envelope::Fatal expects should be silently ignored by serde
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"hi"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_fatal_minimal() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn decode_hello_missing_required_field() {
    // Missing `backend` field
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    let err = JsonlCodec::decode(json).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_hello_mode_defaults_when_absent() {
    // mode has #[serde(default)] so should default to Mapped when omitted
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn decode_fatal_error_code_skip_serializing_if_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    // error_code should not appear in the JSON when None
    assert!(!json.contains("error_code"), "JSON: {json}");
}

#[test]
fn decode_fatal_error_code_present_when_some() {
    let env = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::Internal);
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("error_code"), "JSON: {json}");
}

// =========================================================================
// 9. Clone and Debug impls
// =========================================================================

#[test]
fn envelope_clone() {
    let env = make_hello();
    let cloned = env.clone();
    let j1 = JsonlCodec::encode(&env).unwrap();
    let j2 = JsonlCodec::encode(&cloned).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn envelope_debug() {
    let env = make_hello();
    let debug = format!("{:?}", env);
    assert!(debug.contains("Hello"));
}

#[test]
fn jsonl_codec_debug() {
    let codec = JsonlCodec;
    let debug = format!("{:?}", codec);
    assert!(debug.contains("JsonlCodec"));
}

#[test]
fn protocol_error_debug() {
    let err = ProtocolError::Violation("test".into());
    let debug = format!("{:?}", err);
    assert!(debug.contains("Violation"));
}

// =========================================================================
// 10. ref_id correlation
// =========================================================================

#[test]
fn event_ref_id_preserved_in_roundtrip() {
    let env = make_event("specific-run-id-123");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "specific-run-id-123"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn final_ref_id_preserved_in_roundtrip() {
    let env = make_final("specific-run-id-456");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "specific-run-id-456"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_optional_ref_id_none_roundtrip() {
    let env = make_fatal(None);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_optional_ref_id_some_roundtrip() {
    let env = make_fatal(Some("run-999"));
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-999")),
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// 11. CONTRACT_VERSION usage
// =========================================================================

#[test]
fn hello_uses_contract_version() {
    let env = make_hello();
    match &env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(contract_version, "abp/v0.1");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn contract_version_in_json() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

// =========================================================================
// 12. StreamingCodec
// =========================================================================

#[test]
fn streaming_codec_encode_batch_empty() {
    let batch = StreamingCodec::encode_batch(&[]);
    assert!(batch.is_empty());
}

#[test]
fn streaming_codec_encode_batch_multiple() {
    let envs = vec![make_fatal(None), make_fatal(Some("r1"))];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(batch.lines().count(), 2);
}

#[test]
fn streaming_codec_decode_batch() {
    let envs = vec![make_fatal(None), make_fatal(Some("r1"))];
    let batch = StreamingCodec::encode_batch(&envs);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.is_ok());
    }
}

#[test]
fn streaming_codec_decode_batch_with_errors() {
    let input = format!(
        "{}\nnot valid json\n",
        JsonlCodec::encode(&make_fatal(None)).unwrap().trim()
    );
    let results = StreamingCodec::decode_batch(&input);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn streaming_codec_line_count() {
    let envs = vec![make_fatal(None), make_fatal(None), make_fatal(None)];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 3);
}

#[test]
fn streaming_codec_line_count_with_blanks() {
    assert_eq!(StreamingCodec::line_count("\n\n\n"), 0);
    assert_eq!(StreamingCodec::line_count(""), 0);
}

#[test]
fn streaming_codec_validate_jsonl_all_valid() {
    let envs = vec![make_fatal(None)];
    let batch = StreamingCodec::encode_batch(&envs);
    let errors = StreamingCodec::validate_jsonl(&batch);
    assert!(errors.is_empty());
}

#[test]
fn streaming_codec_validate_jsonl_with_errors() {
    let input = "not valid json\n";
    let errors = StreamingCodec::validate_jsonl(input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 1); // 1-based line number
}

// =========================================================================
// 13. Builder pattern tests
// =========================================================================

#[test]
fn builder_hello_minimal() {
    let env = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello {
            backend,
            contract_version,
            ..
        } => {
            assert_eq!(backend.id, "my-sidecar");
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_missing_backend_returns_error() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("backend"));
}

#[test]
fn builder_hello_full() {
    let env = EnvelopeBuilder::hello()
        .backend("sidecar")
        .version("2.0")
        .adapter_version("1.0")
        .mode(ExecutionMode::Passthrough)
        .capabilities(CapabilityManifest::new())
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { backend, mode, .. } => {
            assert_eq!(backend.id, "sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
            assert_eq!(*mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_fatal_minimal() {
    let env = EnvelopeBuilder::fatal("boom").build().unwrap();
    match &env {
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
fn builder_fatal_with_ref_id() {
    let env = EnvelopeBuilder::fatal("err")
        .ref_id("run-1")
        .build()
        .unwrap();
    match &env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-1")),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn builder_run() {
    let wo = make_work_order();
    let wo_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match &env {
        Envelope::Run { id, .. } => assert_eq!(id, &wo_id),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_run_with_ref_id_override() {
    let wo = make_work_order();
    let env = EnvelopeBuilder::run(wo)
        .ref_id("custom-id")
        .build()
        .unwrap();
    match &env {
        Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_event_missing_ref_id() {
    let event = make_agent_event();
    let err = EnvelopeBuilder::event(event).build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

#[test]
fn builder_event_with_ref_id() {
    let event = make_agent_event();
    let env = EnvelopeBuilder::event(event)
        .ref_id("run-1")
        .build()
        .unwrap();
    match &env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-1"),
        _ => panic!("expected Event"),
    }
}

#[test]
fn builder_final_missing_ref_id() {
    let receipt = make_receipt();
    let err = EnvelopeBuilder::final_receipt(receipt).build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

#[test]
fn builder_final_with_ref_id() {
    let receipt = make_receipt();
    let env = EnvelopeBuilder::final_receipt(receipt)
        .ref_id("run-1")
        .build()
        .unwrap();
    match &env {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-1"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn builder_error_display() {
    let err = BuilderError::MissingField("backend");
    let display = format!("{err}");
    assert!(display.contains("missing required field"));
    assert!(display.contains("backend"));
}

// =========================================================================
// 14. Version module (ProtocolVersion, VersionRange, negotiate_version)
// =========================================================================

#[test]
fn protocol_version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn protocol_version_parse_invalid_format() {
    assert!(matches!(
        ProtocolVersion::parse("invalid"),
        Err(VersionError::InvalidFormat)
    ));
}

#[test]
fn protocol_version_parse_invalid_major() {
    assert!(matches!(
        ProtocolVersion::parse("abp/vXX.1"),
        Err(VersionError::InvalidMajor)
    ));
}

#[test]
fn protocol_version_parse_invalid_minor() {
    assert!(matches!(
        ProtocolVersion::parse("abp/v0.YY"),
        Err(VersionError::InvalidMinor)
    ));
}

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion::parse("abp/v3.7").unwrap();
    assert_eq!(format!("{v}"), "abp/v3.7");
}

#[test]
fn protocol_version_to_string() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.to_string(), "abp/v0.1");
}

#[test]
fn protocol_version_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_is_compatible() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();

    assert!(v01.is_compatible(&v02)); // remote >= local minor
    assert!(!v02.is_compatible(&v01)); // remote < local minor
    assert!(!v01.is_compatible(&v10)); // different major
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
}

#[test]
fn protocol_version_clone_and_eq() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let cloned = v.clone();
    assert_eq!(v, cloned);
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.1").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.2").unwrap()));
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.3").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.0").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.4").unwrap()));
}

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
fn negotiate_version_same_major() {
    let local = ProtocolVersion::parse("abp/v0.2").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.3").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result, ProtocolVersion::parse("abp/v0.2").unwrap());
}

#[test]
fn negotiate_version_different_major() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&local, &remote).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

#[test]
fn negotiate_version_equal() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn version_error_display_invalid_format() {
    let err = VersionError::InvalidFormat;
    let display = format!("{err}");
    assert!(display.contains("invalid version format"));
}

#[test]
fn version_error_display_incompatible() {
    let err = VersionError::Incompatible {
        local: ProtocolVersion::parse("abp/v0.1").unwrap(),
        remote: ProtocolVersion::parse("abp/v1.0").unwrap(),
    };
    let display = format!("{err}");
    assert!(display.contains("incompatible"));
}

// =========================================================================
// 15. Validation
// =========================================================================

#[test]
fn validator_valid_hello() {
    let validator = EnvelopeValidator::new();
    let env = make_hello();
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validator_hello_empty_backend_id() {
    let validator = EnvelopeValidator::new();
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
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id")));
}

#[test]
fn validator_hello_invalid_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "invalid".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

#[test]
fn validator_hello_empty_version() {
    let validator = EnvelopeValidator::new();
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
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn validator_hello_warns_missing_optional_fields() {
    let validator = EnvelopeValidator::new();
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
    let result = validator.validate(&env);
    assert!(result.valid); // warnings only
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field.contains("backend_version"))));
}

#[test]
fn validator_event_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Event {
        ref_id: "".into(),
        event: make_agent_event(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validator_final_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt: make_receipt(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validator_fatal_empty_error() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: None,
        error: "".into(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validator_fatal_warns_missing_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(
        |w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
    ));
}

#[test]
fn validation_error_display() {
    let e = ValidationError::MissingField { field: "x".into() };
    assert!(format!("{e}").contains("missing required field"));

    let e = ValidationError::EmptyField { field: "y".into() };
    assert!(format!("{e}").contains("must not be empty"));

    let e = ValidationError::InvalidVersion {
        version: "bad".into(),
    };
    assert!(format!("{e}").contains("invalid protocol version"));
}

#[test]
fn validation_warning_display() {
    let w = ValidationWarning::DeprecatedField { field: "x".into() };
    assert!(format!("{w}").contains("deprecated"));

    let w = ValidationWarning::LargePayload {
        size: 100,
        max_recommended: 50,
    };
    assert!(format!("{w}").contains("exceeds"));

    let w = ValidationWarning::MissingOptionalField { field: "y".into() };
    assert!(format!("{w}").contains("missing optional"));
}

// =========================================================================
// 16. Sequence validation
// =========================================================================

#[test]
fn sequence_valid() {
    let validator = EnvelopeValidator::new();
    let wo = make_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: run_id.clone(),
            work_order: wo,
        },
        make_event(&run_id),
        make_final(&run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "errors: {errors:?}");
}

#[test]
fn sequence_missing_hello() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_fatal(None)];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn sequence_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello()];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_empty() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_hello_not_first() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_fatal(None), make_hello()];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 })));
}

#[test]
fn sequence_error_display() {
    let e = SequenceError::MissingHello;
    assert!(format!("{e}").contains("missing a Hello"));

    let e = SequenceError::MissingTerminal;
    assert!(format!("{e}").contains("no terminal"));

    let e = SequenceError::MultipleTerminals;
    assert!(format!("{e}").contains("multiple terminal"));

    let e = SequenceError::RefIdMismatch {
        expected: "a".into(),
        found: "b".into(),
    };
    assert!(format!("{e}").contains("ref_id mismatch"));

    let e = SequenceError::OutOfOrderEvents;
    assert!(format!("{e}").contains("outside the Run"));
}

// =========================================================================
// 17. Compression
// =========================================================================

#[test]
fn compression_none_roundtrip() {
    let c = MessageCompressor::new(CompressionAlgorithm::None);
    let data = b"test data";
    let compressed = c.compress(data).unwrap();
    assert_eq!(compressed, data);
    let decompressed = c.decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn compression_gzip_roundtrip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"test gzip roundtrip data";
    let compressed = c.compress(data).unwrap();
    let decompressed = c.decompress(&compressed).unwrap();
    assert_eq!(decompressed, data.as_slice());
}

#[test]
fn compression_zstd_roundtrip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let data = b"test zstd roundtrip data";
    let compressed = c.compress(data).unwrap();
    let decompressed = c.decompress(&compressed).unwrap();
    assert_eq!(decompressed, data.as_slice());
}

#[test]
fn compression_algorithm_accessor() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    assert_eq!(c.algorithm(), CompressionAlgorithm::Gzip);
}

#[test]
fn compressed_message_roundtrip() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let data = b"compressed message test";
    let msg = c.compress_message(data).unwrap();
    assert_eq!(msg.algorithm, CompressionAlgorithm::Gzip);
    assert_eq!(msg.original_size, data.len());
    let decompressed = c.decompress_message(&msg).unwrap();
    assert_eq!(decompressed, data.as_slice());
}

#[test]
fn compression_stats_empty() {
    let stats = CompressionStats::new();
    assert_eq!(stats.total_original, 0);
    assert_eq!(stats.total_compressed, 0);
    assert_eq!(stats.bytes_saved(), 0);
    assert_eq!(stats.compression_ratio(), 0.0);
}

#[test]
fn compression_stats_recording() {
    let mut stats = CompressionStats::new();
    stats.record(1000, 200);
    assert_eq!(stats.total_original, 1000);
    assert_eq!(stats.total_compressed, 200);
    assert_eq!(stats.bytes_saved(), 800);
    assert!((stats.compression_ratio() - 0.2).abs() < f64::EPSILON);
}

#[test]
fn compression_algorithm_serde() {
    let alg = CompressionAlgorithm::Gzip;
    let json = serde_json::to_string(&alg).unwrap();
    assert_eq!(json, r#""gzip""#);
    let decoded: CompressionAlgorithm = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, CompressionAlgorithm::Gzip);
}

#[test]
fn compress_error_too_short() {
    let c = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let err = c.decompress(b"").unwrap_err();
    assert!(matches!(err, CompressError::TooShort));
    assert!(format!("{err}").contains("too short"));
}

#[test]
fn compress_error_algorithm_mismatch() {
    let gzip = MessageCompressor::new(CompressionAlgorithm::Gzip);
    let zstd = MessageCompressor::new(CompressionAlgorithm::Zstd);
    let compressed = gzip.compress(b"test").unwrap();
    let err = zstd.decompress(&compressed).unwrap_err();
    assert!(matches!(err, CompressError::AlgorithmMismatch { .. }));
    assert!(format!("{err}").contains("mismatch"));
}

// =========================================================================
// 18. Batch processing
// =========================================================================

#[test]
fn batch_processor_process_success() {
    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "batch-1".into(),
        envelopes: vec![make_fatal(None), make_fatal(Some("r"))],
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let response = processor.process(request);
    assert_eq!(response.request_id, "batch-1");
    assert_eq!(response.results.len(), 2);
    for result in &response.results {
        assert_eq!(result.status, BatchItemStatus::Success);
        assert!(result.envelope.is_some());
    }
}

#[test]
fn batch_processor_validate_empty_batch() {
    let processor = BatchProcessor::new();
    let request = BatchRequest {
        id: "batch-empty".into(),
        envelopes: vec![],
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let errors = processor.validate_batch(&request);
    assert!(errors.contains(&BatchValidationError::EmptyBatch));
}

#[test]
fn batch_validation_error_display() {
    let e = BatchValidationError::EmptyBatch;
    assert!(format!("{e}").contains("empty"));

    let e = BatchValidationError::TooManyItems {
        count: 2000,
        max: 1000,
    };
    assert!(format!("{e}").contains("2000"));
    assert!(format!("{e}").contains("1000"));

    let e = BatchValidationError::InvalidEnvelope {
        index: 5,
        error: "bad".into(),
    };
    assert!(format!("{e}").contains("index 5"));
}

#[test]
fn batch_item_status_serde() {
    let status = BatchItemStatus::Success;
    let json = serde_json::to_string(&status).unwrap();
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, BatchItemStatus::Success);
}

#[test]
fn batch_item_status_failed_serde() {
    let status = BatchItemStatus::Failed {
        error: "err".into(),
    };
    let json = serde_json::to_string(&status).unwrap();
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, status);
}

#[test]
fn batch_item_status_skipped_serde() {
    let status = BatchItemStatus::Skipped {
        reason: "too big".into(),
    };
    let json = serde_json::to_string(&status).unwrap();
    let decoded: BatchItemStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, status);
}

#[test]
fn max_batch_size_constant() {
    assert_eq!(MAX_BATCH_SIZE, 1000);
}

// =========================================================================
// 19. Router
// =========================================================================

#[test]
fn router_add_and_match_by_type() {
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "fatal".into(),
        destination: "error-handler".into(),
        priority: 1,
    });
    let env = make_fatal(None);
    let matched = router.route(&env);
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().destination, "error-handler");
}

#[test]
fn router_no_match() {
    let router = MessageRouter::new();
    let env = make_hello();
    assert!(router.route(&env).is_none());
}

#[test]
fn router_priority_ordering() {
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "fatal".into(),
        destination: "low-priority".into(),
        priority: 1,
    });
    router.add_route(MessageRoute {
        pattern: "fatal".into(),
        destination: "high-priority".into(),
        priority: 10,
    });
    let env = make_fatal(None);
    let matched = router.route(&env).unwrap();
    assert_eq!(matched.destination, "high-priority");
}

#[test]
fn router_route_all() {
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "fatal".into(),
        destination: "handler".into(),
        priority: 1,
    });
    let envs = vec![make_hello(), make_fatal(None), make_fatal(Some("r"))];
    let matches = router.route_all(&envs);
    assert_eq!(matches.len(), 2);
}

#[test]
fn router_remove_route() {
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "fatal".into(),
        destination: "handler".into(),
        priority: 1,
    });
    assert_eq!(router.route_count(), 1);
    router.remove_route("handler");
    assert_eq!(router.route_count(), 0);
}

#[test]
fn router_ref_id_prefix_match() {
    let mut router = MessageRouter::new();
    router.add_route(MessageRoute {
        pattern: "run-".into(),
        destination: "run-handler".into(),
        priority: 1,
    });
    let env = make_event("run-123");
    let matched = router.route(&env);
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().destination, "run-handler");
}

#[test]
fn route_table_insert_and_lookup() {
    let mut table = RouteTable::new();
    table.insert("hello", "hello-handler");
    table.insert("fatal", "error-handler");
    assert_eq!(table.lookup("hello"), Some("hello-handler"));
    assert_eq!(table.lookup("fatal"), Some("error-handler"));
    assert_eq!(table.lookup("event"), None);
}

#[test]
fn route_table_entries() {
    let mut table = RouteTable::new();
    table.insert("a", "b");
    let entries = table.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get("a").unwrap(), "b");
}

#[test]
fn route_table_serde_roundtrip() {
    let mut table = RouteTable::new();
    table.insert("hello", "h");
    table.insert("fatal", "f");
    let json = serde_json::to_string(&table).unwrap();
    let decoded: RouteTable = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.lookup("hello"), Some("h"));
    assert_eq!(decoded.lookup("fatal"), Some("f"));
}

// =========================================================================
// 20. StreamParser
// =========================================================================

#[test]
fn stream_parser_partial_lines() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None)).unwrap();
    let bytes = line.as_bytes();
    let (first, second) = bytes.split_at(10);
    assert!(parser.feed(first).is_empty());
    let results = parser.feed(second);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_chunk() {
    let mut parser = StreamParser::new();
    let e1 = JsonlCodec::encode(&make_fatal(None)).unwrap();
    let e2 = JsonlCodec::encode(&make_fatal(Some("r"))).unwrap();
    let combined = format!("{e1}{e2}");
    let results = parser.feed(combined.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_blank_lines_skipped() {
    let mut parser = StreamParser::new();
    let e1 = JsonlCodec::encode(&make_fatal(None)).unwrap();
    let input = format!("\n\n{e1}\n\n");
    let results = parser.feed(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_is_empty_and_buffered_len() {
    let mut parser = StreamParser::new();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);

    parser.feed(b"partial");
    assert!(!parser.is_empty());
    assert!(parser.buffered_len() > 0);
}

#[test]
fn stream_parser_reset() {
    let mut parser = StreamParser::new();
    parser.feed(b"partial data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_finish() {
    let mut parser = StreamParser::new();
    let json = JsonlCodec::encode(&make_fatal(None)).unwrap();
    // Feed without trailing newline
    let trimmed = json.trim();
    parser.feed(trimmed.as_bytes());
    assert!(parser.buffered_len() > 0);
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_with_max_line_len() {
    let mut parser = StreamParser::with_max_line_len(10);
    // Create a line longer than 10 bytes
    let long_line = format!("{}\n", "x".repeat(20));
    let results = parser.feed(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_default_trait() {
    let p = StreamParser::default();
    assert!(p.is_empty());
}

// =========================================================================
// 21. AgentEvent variants roundtrip through Envelope
// =========================================================================

#[test]
fn event_assistant_message_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_assistant_delta_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_run_started_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_run_completed_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_tool_call_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp/file.txt"}),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_tool_result_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            output: serde_json::json!("file contents"),
            is_error: false,
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_file_changed_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_command_executed_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_warning_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "be careful".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_error_roundtrip() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "something failed".into(),
            error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// =========================================================================
// 22. Full protocol flow encode/decode
// =========================================================================

#[test]
fn full_protocol_flow_roundtrip() {
    let wo = make_work_order();
    let run_id = wo.id.to_string();

    let sequence = vec![
        make_hello(),
        Envelope::Run {
            id: run_id.clone(),
            work_order: wo,
        },
        make_event(&run_id),
        make_event(&run_id),
        make_final(&run_id),
    ];

    // Encode all as JSONL
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &sequence).unwrap();
    let jsonl = String::from_utf8(buf).unwrap();

    // Decode back via stream
    let reader = BufReader::new(jsonl.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 5);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Event { .. }));
    assert!(matches!(decoded[4], Envelope::Final { .. }));
}

#[test]
fn full_protocol_flow_with_fatal() {
    let sequence = vec![make_hello(), make_fatal(None)];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &sequence).unwrap();
    let jsonl = String::from_utf8(buf).unwrap();

    let reader = BufReader::new(jsonl.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Fatal { .. }));
}

// =========================================================================
// 23. Misc edge cases
// =========================================================================

#[test]
fn envelope_clone_all_variants() {
    let envs = vec![
        make_hello(),
        make_run(),
        make_event("r"),
        make_final("r"),
        make_fatal(None),
        make_fatal(Some("r")),
    ];
    for env in &envs {
        let _ = env.clone();
    }
}

#[test]
fn envelope_debug_all_variants() {
    let envs = vec![
        make_hello(),
        make_run(),
        make_event("r"),
        make_final("r"),
        make_fatal(None),
    ];
    for env in &envs {
        let debug = format!("{:?}", env);
        assert!(!debug.is_empty());
    }
}

#[test]
fn json_field_names_are_snake_case() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains("contract_version"));
    assert!(!json.contains("contractVersion"));
}

#[test]
fn json_fatal_ref_id_is_null_when_none() {
    let env = make_fatal(None);
    let json = JsonlCodec::encode(&env).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["ref_id"].is_null());
}

#[test]
fn json_fatal_ref_id_is_string_when_some() {
    let env = make_fatal(Some("run-1"));
    let json = JsonlCodec::encode(&env).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ref_id"].as_str().unwrap(), "run-1");
}

#[test]
fn decode_preserves_unicode() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "こんにちは世界 🌍".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "こんにちは世界 🌍"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_preserves_special_chars() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "line1\nline2\ttab\"quote".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "line1\nline2\ttab\"quote"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_empty_error_string() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, ""),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_very_long_error_string() {
    let long_msg = "x".repeat(100_000);
    let env = Envelope::Fatal {
        ref_id: None,
        error: long_msg.clone(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, long_msg),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn batch_request_serde_roundtrip() {
    let request = BatchRequest {
        id: "b1".into(),
        envelopes: vec![make_fatal(None)],
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&request).unwrap();
    let decoded: BatchRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, "b1");
    assert_eq!(decoded.envelopes.len(), 1);
}

#[test]
fn batch_response_serde_roundtrip() {
    let response = BatchResponse {
        request_id: "b1".into(),
        results: vec![BatchResult {
            index: 0,
            status: BatchItemStatus::Success,
            envelope: Some(make_fatal(None)),
        }],
        total_duration_ms: 42,
    };
    let json = serde_json::to_string(&response).unwrap();
    let decoded: BatchResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.request_id, "b1");
    assert_eq!(decoded.total_duration_ms, 42);
}

#[test]
fn message_route_serde_roundtrip() {
    let route = MessageRoute {
        pattern: "fatal".into(),
        destination: "handler".into(),
        priority: 10,
    };
    let json = serde_json::to_string(&route).unwrap();
    let decoded: MessageRoute = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.pattern, "fatal");
    assert_eq!(decoded.destination, "handler");
    assert_eq!(decoded.priority, 10);
}

#[test]
fn compressed_message_serde_roundtrip() {
    let c = MessageCompressor::new(CompressionAlgorithm::None);
    let msg = c.compress_message(b"test data").unwrap();
    let json = serde_json::to_string(&msg).unwrap();
    let decoded: CompressedMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.algorithm, CompressionAlgorithm::None);
    assert_eq!(decoded.original_size, msg.original_size);
}

#[test]
fn protocol_version_serde_roundtrip() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let json = serde_json::to_string(&v).unwrap();
    let decoded: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, v);
}

#[test]
fn version_range_serde_roundtrip() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    let json = serde_json::to_string(&range).unwrap();
    let decoded: VersionRange = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, range);
}

#[test]
fn validation_result_new_is_valid() {
    // Indirectly test: a valid envelope should produce valid result
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&make_fatal(Some("r")));
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validator_run_empty_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Run {
        id: "".into(),
        work_order: make_work_order(),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "id")));
}

#[test]
fn validator_default() {
    let validator = EnvelopeValidator::default();
    let result = validator.validate(&make_hello());
    assert!(result.valid);
}

#[test]
fn batch_processor_default() {
    let processor = BatchProcessor::default();
    let request = BatchRequest {
        id: "b".into(),
        envelopes: vec![make_fatal(None)],
        created_at: "now".into(),
    };
    let response = processor.process(request);
    assert_eq!(response.results.len(), 1);
}

#[test]
fn router_default() {
    let router = MessageRouter::default();
    assert_eq!(router.route_count(), 0);
}

#[test]
fn route_table_default() {
    let table = RouteTable::default();
    assert!(table.entries().is_empty());
}

#[test]
fn stream_parser_clone() {
    let mut parser = StreamParser::new();
    parser.feed(b"partial");
    let cloned = parser.clone();
    assert_eq!(cloned.buffered_len(), parser.buffered_len());
}

#[test]
fn protocol_version_hash() {
    use std::collections::HashSet;
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let mut set = HashSet::new();
    set.insert(v1);
    set.insert(v2);
    assert_eq!(set.len(), 1);
}
