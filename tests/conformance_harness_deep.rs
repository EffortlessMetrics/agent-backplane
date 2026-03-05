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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Deep conformance test harness verifying that ABP conforms to its own
//! protocol specification (`docs/sidecar_protocol.md`).
//!
//! Covers: protocol version, envelope format, handshake sequence, run
//! lifecycle, error handling, receipt conformance, error taxonomy,
//! capability negotiation, deterministic serialization, and execution modes.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_capability::negotiate_capabilities;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionLane, ExecutionMode, Outcome, ReceiptBuilder, RuntimeConfig, SupportLevel,
    WorkOrderBuilder, WorkspaceMode, canonical_json, receipt_hash, sha256_hex,
};
use abp_error_taxonomy::{ErrorCategory, ErrorCode};
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::version::{ProtocolVersion, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

// =========================================================================
// Helpers
// =========================================================================

fn fixed_ts() -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap()
}

fn ts_offset(ms: i64) -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp_millis(1_700_000_000_000 + ms).unwrap()
}

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_event(ref_id: &str, kind: AgentEventKind, offset_ms: i64) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: ts_offset(offset_ms),
            kind,
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-sidecar").build();
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

fn make_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("conformance test task").build()
}

fn make_run_envelope(run_id: &str) -> Envelope {
    Envelope::Run {
        id: run_id.into(),
        work_order: make_work_order(),
    }
}

fn sample_sequence(ref_id: &str) -> Vec<Envelope> {
    vec![
        make_hello(),
        make_run_envelope(ref_id),
        make_event(
            ref_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_event(
            ref_id,
            AgentEventKind::AssistantDelta { text: "hi".into() },
            1,
        ),
        make_event(
            ref_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            2,
        ),
        make_final(ref_id),
    ]
}

// =========================================================================
// 1. Protocol Version Conformance (10 tests)
// =========================================================================

#[test]
fn version_contract_version_format() {
    // Spec: version format is "abp/vMAJOR.MINOR"
    assert!(
        CONTRACT_VERSION.starts_with("abp/v"),
        "CONTRACT_VERSION must start with 'abp/v'"
    );
    assert!(
        parse_version(CONTRACT_VERSION).is_some(),
        "CONTRACT_VERSION must be parseable"
    );
}

#[test]
fn version_contract_version_is_v0_1() {
    // Spec: Contract version: `abp/v0.1`
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn version_parse_valid_versions() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn version_parse_invalid_versions() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("abp/vx.y"), None);
}

#[test]
fn version_compatible_same_major() {
    // Spec: compatible if same major version
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn version_incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn version_protocol_version_struct_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn version_negotiate_same_major_picks_minimum() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn version_negotiate_different_major_fails() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&v01, &v10).is_err());
}

#[test]
fn version_hello_envelope_carries_contract_version() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 2. Envelope Format Conformance (15 tests)
// =========================================================================

#[test]
fn envelope_hello_has_t_discriminator() {
    // Spec: discriminator field is "t" (not "type")
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(
        json.contains("\"t\":\"hello\""),
        "must have t=hello: {json}"
    );
}

#[test]
fn envelope_run_has_t_discriminator() {
    let run = make_run_envelope("run-1");
    let json = JsonlCodec::encode(&run).unwrap();
    assert!(json.contains("\"t\":\"run\""), "must have t=run: {json}");
}

#[test]
fn envelope_event_has_t_discriminator() {
    let event = make_event(
        "r",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        0,
    );
    let json = JsonlCodec::encode(&event).unwrap();
    assert!(
        json.contains("\"t\":\"event\""),
        "must have t=event: {json}"
    );
}

#[test]
fn envelope_final_has_t_discriminator() {
    let fin = make_final("r");
    let json = JsonlCodec::encode(&fin).unwrap();
    assert!(
        json.contains("\"t\":\"final\""),
        "must have t=final: {json}"
    );
}

#[test]
fn envelope_fatal_has_t_discriminator() {
    let fatal = make_fatal(Some("r"), "boom");
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(
        json.contains("\"t\":\"fatal\""),
        "must have t=fatal: {json}"
    );
}

#[test]
fn envelope_hello_required_fields() {
    // Spec: hello requires contract_version, backend, capabilities
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains("\"contract_version\""));
    assert!(json.contains("\"backend\""));
    assert!(json.contains("\"capabilities\""));
}

#[test]
fn envelope_run_required_fields() {
    // Spec: run requires id, work_order
    let run = make_run_envelope("run-1");
    let json = JsonlCodec::encode(&run).unwrap();
    assert!(json.contains("\"id\""));
    assert!(json.contains("\"work_order\""));
}

#[test]
fn envelope_event_required_fields() {
    // Spec: event requires ref_id, event
    let event = make_event(
        "r",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        0,
    );
    let json = JsonlCodec::encode(&event).unwrap();
    assert!(json.contains("\"ref_id\""));
    // The event field is flattened into the envelope but its content is present
    assert!(json.contains("\"ts\""));
}

#[test]
fn envelope_final_required_fields() {
    // Spec: final requires ref_id, receipt
    let fin = make_final("r");
    let json = JsonlCodec::encode(&fin).unwrap();
    assert!(json.contains("\"ref_id\""));
    assert!(json.contains("\"receipt\""));
}

#[test]
fn envelope_fatal_ref_id_optional() {
    // Spec: fatal ref_id is optional
    let fatal = make_fatal(None, "crash");
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.contains("\"ref_id\":null"));
    assert!(json.contains("\"error\":\"crash\""));
}

#[test]
fn envelope_jsonl_newline_terminated() {
    // Spec: every message is terminated by \n
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(
        line.ends_with('\n'),
        "JSONL line must be newline-terminated"
    );
}

#[test]
fn envelope_round_trip_hello() {
    let original = make_hello();
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_round_trip_event() {
    let original = make_event(
        "r1",
        AgentEventKind::AssistantDelta { text: "hi".into() },
        0,
    );
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert!(matches!(event.kind, AgentEventKind::AssistantDelta { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn envelope_round_trip_fatal() {
    let original = make_fatal(Some("r1"), "test error");
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "test error");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_mode_defaults_to_mapped() {
    // Spec: mode defaults to "mapped" if absent
    let json =
        r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 3. Handshake Sequence Conformance (10 tests)
// =========================================================================

#[test]
fn handshake_hello_must_be_first() {
    // Spec: sidecar MUST send hello as very first stdout line
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_final("r"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello)),
        "should detect missing hello"
    );
}

#[test]
fn handshake_hello_not_at_position_zero() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_run_envelope("r"),
        make_hello(),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_final("r"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "should detect hello not first"
    );
}

#[test]
fn handshake_hello_backend_id_required() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&hello);
    assert!(!result.valid, "empty backend.id should be invalid");
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn handshake_hello_contract_version_required() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: String::new(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "contract_version"
    )));
}

#[test]
fn handshake_hello_bad_version_format() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: "not-valid".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn handshake_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps.clone(),
    );
    match &hello {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 2);
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn handshake_hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(
        json.contains("\"passthrough\""),
        "mode should be serialized"
    );
}

#[test]
fn handshake_hello_mapped_mode_explicit() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Mapped,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"mapped\""), "mode should be serialized");
}

#[test]
fn handshake_builder_sets_contract_version() {
    // EnvelopeBuilder::hello() should embed CONTRACT_VERSION automatically
    let env = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn handshake_valid_hello_passes_validation() {
    let validator = EnvelopeValidator::new();
    let hello = make_hello();
    let result = validator.validate(&hello);
    assert!(result.valid, "valid hello should pass: {:?}", result.errors);
}

// =========================================================================
// 4. Run Lifecycle Conformance (15 tests)
// =========================================================================

#[test]
fn lifecycle_valid_sequence_accepted() {
    // Spec: Hello → Run → Event* → Final
    let validator = EnvelopeValidator::new();
    let seq = sample_sequence("run-1");
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}

#[test]
fn lifecycle_ref_id_must_match_run_id() {
    // Spec: event/final ref_id MUST match run.id
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("run-1"),
        make_event(
            "run-WRONG",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_final("run-1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "should detect ref_id mismatch"
    );
}

#[test]
fn lifecycle_final_ref_id_mismatch_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("run-1"),
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_final("run-OTHER"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn lifecycle_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("r"),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn lifecycle_multiple_terminals_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("r"),
        make_final("r"),
        make_fatal(Some("r"), "extra"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals))
    );
}

#[test]
fn lifecycle_empty_sequence_detected() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn lifecycle_event_before_run_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_run_envelope("r"),
        make_final("r"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents)),
        "event before run should be detected"
    );
}

#[test]
fn lifecycle_zero_events_valid() {
    // hello → run → final (no events) is valid
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello(), make_run_envelope("r"), make_final("r")];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "zero events is valid: {errors:?}");
}

#[test]
fn lifecycle_many_events_before_final() {
    let ref_id = "run-big";
    let mut seq = vec![make_hello(), make_run_envelope(ref_id)];
    for i in 0..20 {
        seq.push(make_event(
            ref_id,
            AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            },
            i,
        ));
    }
    seq.push(make_final(ref_id));
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "many events should be valid: {errors:?}");
}

#[test]
fn lifecycle_fatal_ending_valid() {
    // Spec: fatal is a valid terminal
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("r"),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_fatal(Some("r"), "crash"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "fatal ending should be valid: {errors:?}"
    );
}

#[test]
fn lifecycle_jsonl_stream_round_trip() {
    let seq = sample_sequence("rnd-trip");
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &seq).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), seq.len());
}

#[test]
fn lifecycle_decode_stream_skips_blank_lines() {
    let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
    let fatal_line = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("{hello_line}\n\n{fatal_line}");
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn lifecycle_event_validator_checks_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let event = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
    };
    let result = validator.validate(&event);
    assert!(!result.valid, "empty ref_id should be invalid");
}

#[test]
fn lifecycle_run_validator_checks_empty_id() {
    let validator = EnvelopeValidator::new();
    let mut wo = make_work_order();
    wo.task = "test".into();
    let run = Envelope::Run {
        id: String::new(),
        work_order: wo,
    };
    let result = validator.validate(&run);
    assert!(!result.valid, "empty run id should be invalid");
}

#[test]
fn lifecycle_run_validator_checks_empty_task() {
    let validator = EnvelopeValidator::new();
    let mut wo = make_work_order();
    wo.task = String::new();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let result = validator.validate(&run);
    assert!(!result.valid, "empty task should be invalid");
}

// =========================================================================
// 5. Error Handling Conformance (10 tests)
// =========================================================================

#[test]
fn error_fatal_terminates_run() {
    // Spec: fatal is an unrecoverable error
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run_envelope("r"),
        make_fatal(Some("r"), "unrecoverable"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "fatal should be a valid terminal: {errors:?}"
    );
}

#[test]
fn error_fatal_without_ref_id_valid() {
    // Spec: ref_id is optional on fatal (error may occur before run assigned)
    let fatal = make_fatal(None, "initialization failure");
    let json = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "initialization failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn error_fatal_with_ref_id() {
    let fatal = make_fatal(Some("run-42"), "out of memory");
    match &fatal {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn error_invalid_json_produces_protocol_error() {
    // Spec: invalid JSON → ProtocolError::Json
    let result = JsonlCodec::decode("not valid json");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn error_protocol_violation_variant() {
    let err = ProtocolError::Violation("test violation".into());
    assert!(err.to_string().contains("protocol violation"));
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn error_unexpected_message_variant() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert!(err.to_string().contains("unexpected message"));
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn error_fatal_envelope_error_code_accessor() {
    let fatal = Envelope::fatal_with_code(
        Some("r".into()),
        "bad request",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn error_non_fatal_envelope_no_error_code() {
    let hello = make_hello();
    assert!(hello.error_code().is_none());
}

#[test]
fn error_fatal_validator_empty_error_message() {
    let validator = EnvelopeValidator::new();
    let fatal = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: String::new(),
        error_code: None,
    };
    let result = validator.validate(&fatal);
    assert!(!result.valid, "empty error message should be invalid");
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "error"
    )));
}

#[test]
fn error_fatal_missing_ref_id_warns() {
    let validator = EnvelopeValidator::new();
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "crash".into(),
        error_code: None,
    };
    let result = validator.validate(&fatal);
    // Should be valid (ref_id is optional) but with a warning
    assert!(result.valid, "fatal without ref_id is still valid");
    assert!(
        !result.warnings.is_empty(),
        "should warn about missing optional ref_id"
    );
}

// =========================================================================
// 6. Receipt Conformance (15 tests)
// =========================================================================

#[test]
fn receipt_builder_sets_contract_version() {
    let receipt = ReceiptBuilder::new("test-backend").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_default_outcome_complete() {
    let receipt = ReceiptBuilder::new("test").build();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn receipt_builder_custom_outcome() {
    let receipt = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[test]
fn receipt_hash_is_sha256_hex() {
    let receipt = ReceiptBuilder::new("test").build();
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 chars");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_deterministic() {
    // Same receipt → same hash
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_hash_populates_sha256() {
    let receipt = ReceiptBuilder::new("test").build().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_excludes_own_field() {
    // Gotcha: receipt_sha256 is set to null before hashing
    let receipt1 = ReceiptBuilder::new("test").build();
    let hash_without = receipt_hash(&receipt1).unwrap();

    let mut receipt2 = receipt1.clone();
    receipt2.receipt_sha256 = Some("some-previous-hash".into());
    let hash_with = receipt_hash(&receipt2).unwrap();

    assert_eq!(
        hash_without, hash_with,
        "hash must not depend on receipt_sha256 field"
    );
}

#[test]
fn receipt_with_hash_verifiable() {
    let receipt = ReceiptBuilder::new("test").build().with_hash().unwrap();
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[test]
fn receipt_different_data_different_hash() {
    let r1 = ReceiptBuilder::new("backend-a").build();
    let r2 = ReceiptBuilder::new("backend-b").build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "different receipts should have different hashes");
}

#[test]
fn receipt_canonical_json_sorts_keys() {
    let v = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let canon = canonical_json(&v).unwrap();
    assert!(
        canon.starts_with("{\"a\":"),
        "keys should be sorted: {canon}"
    );
}

#[test]
fn receipt_sha256_hex_correct() {
    let hash = sha256_hex(b"hello");
    assert_eq!(hash.len(), 64);
    // Known SHA-256 of "hello"
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn receipt_required_meta_fields() {
    let receipt = ReceiptBuilder::new("test").build();
    assert!(!receipt.meta.contract_version.is_empty());
    assert!(!receipt.backend.id.is_empty());
    // run_id and work_order_id are UUIDs (always present)
    assert_ne!(receipt.meta.run_id, Uuid::nil());
}

#[test]
fn receipt_mode_serializes() {
    let r1 = ReceiptBuilder::new("test")
        .mode(ExecutionMode::Passthrough)
        .build();
    let json = serde_json::to_string(&r1).unwrap();
    assert!(json.contains("\"passthrough\""));

    let r2 = ReceiptBuilder::new("test")
        .mode(ExecutionMode::Mapped)
        .build();
    let json2 = serde_json::to_string(&r2).unwrap();
    assert!(json2.contains("\"mapped\""));
}

#[test]
fn receipt_in_final_envelope_round_trips() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: receipt.clone(),
    };
    let json = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt: r } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
            assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
            assert_eq!(r.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_outcome_variants_serde() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, outcome);
    }
}

// =========================================================================
// 7. Error Taxonomy Conformance (8 tests)
// =========================================================================

fn all_error_codes() -> Vec<ErrorCode> {
    vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::Internal,
    ]
}

#[test]
fn taxonomy_all_codes_have_snake_case_as_str() {
    for code in all_error_codes() {
        let s = code.as_str();
        assert!(!s.is_empty(), "as_str() empty for {:?}", code);
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
            "as_str() '{}' for {:?} is not snake_case",
            s,
            code
        );
    }
}

#[test]
fn taxonomy_all_codes_have_category() {
    for code in all_error_codes() {
        let _ = code.category();
    }
}

#[test]
fn taxonomy_all_codes_have_message() {
    for code in all_error_codes() {
        let msg = code.message();
        assert!(!msg.is_empty(), "message() empty for {:?}", code);
    }
}

#[test]
fn taxonomy_serde_roundtrip_all_codes() {
    for code in all_error_codes() {
        let json = serde_json::to_string(&code).unwrap();
        let code2: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, code2, "roundtrip failed for {:?}", code);
    }
}

#[test]
fn taxonomy_as_str_matches_serde_output() {
    for code in all_error_codes() {
        let serde_str = serde_json::to_value(&code)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(code.as_str(), serde_str, "as_str() != serde for {:?}", code);
    }
}

#[test]
fn taxonomy_protocol_codes_have_protocol_category() {
    let protocol_codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
    ];
    for code in protocol_codes {
        assert_eq!(
            code.category(),
            ErrorCategory::Protocol,
            "{:?} should be Protocol category",
            code
        );
    }
}

#[test]
fn taxonomy_retryable_only_backend_transients() {
    let retryable: Vec<ErrorCode> = all_error_codes()
        .into_iter()
        .filter(|c| c.is_retryable())
        .collect();
    for code in &retryable {
        assert_eq!(
            code.category(),
            ErrorCategory::Backend,
            "{:?} is retryable but not Backend",
            code
        );
    }
    assert!(retryable.contains(&ErrorCode::BackendTimeout));
    assert!(retryable.contains(&ErrorCode::BackendUnavailable));
}

#[test]
fn taxonomy_contract_codes_have_contract_category() {
    let contract_codes = vec![
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
    ];
    for code in contract_codes {
        assert_eq!(
            code.category(),
            ErrorCategory::Contract,
            "{:?} should be Contract category",
            code
        );
    }
}

// =========================================================================
// 8. Capability Negotiation Conformance (5 tests)
// =========================================================================

#[test]
fn capability_negotiate_empty_is_viable() {
    let manifest = CapabilityManifest::new();
    let result = negotiate_capabilities(&[], &manifest);
    assert!(result.is_viable());
}

#[test]
fn capability_negotiate_native_classified() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    let result = negotiate_capabilities(&[Capability::ToolRead], &manifest);
    assert_eq!(result.native.len(), 1);
    assert!(result.unsupported.is_empty());
}

#[test]
fn capability_negotiate_emulated_classified() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolBash, SupportLevel::Emulated);
    let result = negotiate_capabilities(&[Capability::ToolBash], &manifest);
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn capability_negotiate_unsupported_classified() {
    let manifest = CapabilityManifest::new();
    let result = negotiate_capabilities(&[Capability::Vision], &manifest);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn capability_negotiate_mixed_classification() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolEdit, SupportLevel::Emulated);
    let required = vec![
        Capability::Streaming,
        Capability::ToolEdit,
        Capability::Audio,
    ];
    let result = negotiate_capabilities(&required, &manifest);
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
    assert_eq!(result.total(), 3);
}

// =========================================================================
// 9. AgentEvent Variant Serde (5 tests)
// =========================================================================

#[test]
fn event_kind_tool_call_serde_roundtrip() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "foo.rs"}),
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "read_file");
    let e2: AgentEvent = serde_json::from_value(v).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn event_kind_tool_result_serde_roundtrip() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("ok"),
            is_error: false,
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["is_error"], false);
}

#[test]
fn event_kind_file_changed_serde_roundtrip() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "file_changed");
    let e2: AgentEvent = serde_json::from_value(v).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn event_kind_error_with_code_serde() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: Some(ErrorCode::Internal),
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "error");
    assert!(v["error_code"].is_string());
}

#[test]
fn event_kind_error_without_code_omits_field() {
    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "error");
    // skip_serializing_if means the field should be absent
    assert!(v.get("error_code").is_none());
}

// =========================================================================
// 10. Deterministic Serialization (5 tests)
// =========================================================================

#[test]
fn deterministic_btreemap_vendor_config_ordered() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("zeta".into(), serde_json::json!(1));
    config.vendor.insert("alpha".into(), serde_json::json!(2));
    config.vendor.insert("mu".into(), serde_json::json!(3));

    let json = serde_json::to_string(&config).unwrap();
    let alpha_pos = json.find("alpha").unwrap();
    let mu_pos = json.find("mu").unwrap();
    let zeta_pos = json.find("zeta").unwrap();
    assert!(alpha_pos < mu_pos);
    assert!(mu_pos < zeta_pos);
}

#[test]
fn deterministic_capability_manifest_key_ordering() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolWrite, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let json = serde_json::to_string(&manifest).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "BTreeMap keys must be sorted");
}

#[test]
fn deterministic_receipt_serialization_stable() {
    let start = chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
    let finish = chrono::DateTime::from_timestamp_millis(1_700_000_001_000).unwrap();
    let wo_id = Uuid::from_u128(200);

    // Build once, then serialize twice — must be identical.
    let receipt = ReceiptBuilder::new("det-test")
        .work_order_id(wo_id)
        .started_at(start)
        .finished_at(finish)
        .outcome(Outcome::Complete)
        .build();

    let json1 = serde_json::to_string(&receipt).unwrap();
    let json2 = serde_json::to_string(&receipt).unwrap();
    assert_eq!(json1, json2, "same receipt must serialize identically");
}

#[test]
fn deterministic_envelope_serialization_stable() {
    let build = || {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let env = Envelope::hello(
            BackendIdentity {
                id: "det".into(),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            caps,
        );
        JsonlCodec::encode(&env).unwrap()
    };

    assert_eq!(build(), build(), "same envelope must encode identically");
}

#[test]
fn deterministic_agent_event_ext_btreemap_ordered() {
    let mut ext = BTreeMap::new();
    ext.insert("z_key".into(), serde_json::json!(1));
    ext.insert("a_key".into(), serde_json::json!(2));
    ext.insert("m_key".into(), serde_json::json!(3));

    let e = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: Some(ext),
    };

    let json = serde_json::to_string(&e).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

// =========================================================================
// 11. Execution Mode Invariants (4 tests)
// =========================================================================

#[test]
fn mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn mode_passthrough_serializes() {
    let v = serde_json::to_value(ExecutionMode::Passthrough).unwrap();
    assert_eq!(v, "passthrough");
}

#[test]
fn mode_mapped_serializes() {
    let v = serde_json::to_value(ExecutionMode::Mapped).unwrap();
    assert_eq!(v, "mapped");
}

#[test]
fn mode_roundtrip_all_variants() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let json = serde_json::to_string(&mode).unwrap();
        let m2: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, m2);
    }
}

// =========================================================================
// 12. WorkOrder Schema Extras (3 tests)
// =========================================================================

#[test]
fn work_order_id_unique_per_build() {
    let a = WorkOrderBuilder::new("a").build();
    let b = WorkOrderBuilder::new("b").build();
    assert_ne!(a.id, b.id);
}

#[test]
fn work_order_lane_serializes_snake_case() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("patch_first"));
}

#[test]
fn work_order_workspace_mode_serializes_snake_case() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("pass_through"));
}
