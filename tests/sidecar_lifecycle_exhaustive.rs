#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive sidecar protocol lifecycle tests covering the full Hello → Run →
//! Event* → Final/Fatal flow, missing-hello detection, invalid envelopes,
//! ref_id correlation, large/unicode/empty payloads, concurrent sidecars,
//! timeout handling, and process lifecycle.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::time::Duration;

use abp_core::*;
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use uuid::Uuid;

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

fn test_identity_with_id(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("0.1.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn rich_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Native);
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

fn test_work_order_with_task(task: &str) -> WorkOrder {
    let mut wo = test_work_order();
    wo.task = task.into();
    wo
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

fn test_event_kind(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn test_event() -> AgentEvent {
    test_event_kind(AgentEventKind::RunStarted {
        message: "started".into(),
    })
}

fn make_hello() -> Envelope {
    Envelope::hello(test_identity(), test_capabilities())
}

fn make_hello_with_mode(mode: ExecutionMode) -> Envelope {
    Envelope::hello_with_mode(test_identity(), test_capabilities(), mode)
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

fn make_event_with_kind(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: test_event_kind(kind),
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

fn round_trip(envelope: &Envelope) -> Envelope {
    let json = JsonlCodec::encode(envelope).expect("encode");
    JsonlCodec::decode(json.trim()).expect("decode")
}

/// Encode a sequence of envelopes to a JSONL string.
fn encode_sequence(envelopes: &[Envelope]) -> String {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, envelopes).unwrap();
    String::from_utf8(buf).unwrap()
}

/// Decode a JSONL string into a Vec of envelopes.
fn decode_sequence(jsonl: &str) -> Vec<Envelope> {
    let reader = BufReader::new(Cursor::new(jsonl));
    JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn validator() -> EnvelopeValidator {
    EnvelopeValidator::new()
}

// ===========================================================================
// 1. Happy-path: Hello → Run → Event* → Final
// ===========================================================================

#[test]
fn lifecycle_hello_run_single_event_final() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_hello_run_no_events_final() {
    let seq = vec![make_hello(), make_run("r1"), make_final("r1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_hello_run_many_events_final() {
    let mut seq = vec![make_hello(), make_run("r1")];
    for _ in 0..50 {
        seq.push(make_event("r1"));
    }
    seq.push(make_final("r1"));
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_hello_run_event_types_final() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        AgentEventKind::AssistantDelta { text: "hel".into() },
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "foo.rs"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("contents"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "watch out".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ];
    let mut seq = vec![make_hello(), make_run("r1")];
    for k in kinds {
        seq.push(make_event_with_kind("r1", k));
    }
    seq.push(make_final("r1"));
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_roundtrip_full_sequence() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let jsonl = encode_sequence(&seq);
    let decoded = decode_sequence(&jsonl);
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// ===========================================================================
// 2. Hello → Run → Event* → Fatal sequence
// ===========================================================================

#[test]
fn lifecycle_hello_run_event_fatal() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_fatal(Some("r1"), "out of memory"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_hello_run_fatal_no_events() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("r1"), "immediate failure"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_fatal_without_ref_id() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(None, "unknown error"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn lifecycle_fatal_with_error_code() {
    let fatal = Envelope::fatal_with_code(
        Some("r1".into()),
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    let seq = vec![make_hello(), make_run("r1"), fatal];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn fatal_envelope_error_code_extraction() {
    let fatal = Envelope::fatal_with_code(
        Some("r1".into()),
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::BackendTimeout)
    );
}

#[test]
fn fatal_envelope_without_error_code_returns_none() {
    let fatal = make_fatal(Some("r1"), "boom");
    assert_eq!(fatal.error_code(), None);
}

#[test]
fn non_fatal_envelope_error_code_returns_none() {
    let hello = make_hello();
    assert_eq!(hello.error_code(), None);
}

// ===========================================================================
// 3. Missing hello detection
// ===========================================================================

#[test]
fn missing_hello_detected_in_empty_sequence() {
    let errors = validator().validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn missing_hello_detected_when_run_is_first() {
    let seq = vec![make_run("r1"), make_event("r1"), make_final("r1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn hello_not_first_detected() {
    let seq = vec![make_run("r1"), make_hello(), make_final("r1")];
    let errors = validator().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "expected HelloNotFirst, got: {errors:?}"
    );
}

#[test]
fn hello_at_position_two_detected() {
    let seq = vec![
        make_run("r1"),
        make_event("r1"),
        make_hello(),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 2 })));
}

#[test]
fn missing_hello_with_only_fatal() {
    let seq = vec![make_fatal(None, "crash")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

// ===========================================================================
// 4. Invalid envelope rejection
// ===========================================================================

#[test]
fn decode_invalid_json_returns_error() {
    let err = JsonlCodec::decode("this is not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_empty_string_returns_error() {
    let err = JsonlCodec::decode("").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_missing_tag_field_returns_error() {
    let err = JsonlCodec::decode(r#"{"ref_id":"r1","error":"boom"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_unknown_tag_value_returns_error() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_partial_json_returns_error() {
    let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_array_instead_of_object_returns_error() {
    let err = JsonlCodec::decode("[1,2,3]").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_null_returns_error() {
    let err = JsonlCodec::decode("null").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_number_returns_error() {
    let err = JsonlCodec::decode("42").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn validate_hello_with_empty_backend_id() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validator().validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn validate_hello_with_empty_contract_version() {
    let hello = Envelope::Hello {
        contract_version: "".into(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validator().validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn validate_hello_with_invalid_version_format() {
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = validator().validate(&hello);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

#[test]
fn validate_run_with_empty_id() {
    let run = Envelope::Run {
        id: "".into(),
        work_order: test_work_order(),
    };
    let result = validator().validate(&run);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "id")));
}

#[test]
fn validate_run_with_empty_task() {
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: test_work_order_with_task(""),
    };
    let result = validator().validate(&run);
    assert!(!result.valid);
}

#[test]
fn validate_event_with_empty_ref_id() {
    let event = Envelope::Event {
        ref_id: "".into(),
        event: test_event(),
    };
    let result = validator().validate(&event);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "ref_id")));
}

#[test]
fn validate_final_with_empty_ref_id() {
    let f = Envelope::Final {
        ref_id: "".into(),
        receipt: test_receipt(Uuid::nil()),
    };
    let result = validator().validate(&f);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_with_empty_error() {
    let fatal = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "".into(),
        error_code: None,
    };
    let result = validator().validate(&fatal);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "error")));
}

#[test]
fn validate_fatal_warns_on_missing_ref_id() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let result = validator().validate(&fatal);
    assert!(result.valid);
    assert!(!result.warnings.is_empty());
}

#[test]
fn validate_valid_hello_passes() {
    let result = validator().validate(&make_hello());
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

// ===========================================================================
// 5. ref_id correlation enforcement
// ===========================================================================

#[test]
fn ref_id_mismatch_event_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("wrong-ref"),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch {
            expected,
            found
        } if expected == "r1" && found == "wrong-ref"
    )));
}

#[test]
fn ref_id_mismatch_final_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("wrong-ref"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn ref_id_mismatch_fatal_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("wrong-ref"), "error"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn ref_id_correct_event_and_final_passes() {
    let seq = vec![
        make_hello(),
        make_run("run-abc"),
        make_event("run-abc"),
        make_event("run-abc"),
        make_final("run-abc"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn ref_id_uuid_format_accepted() {
    let id = Uuid::new_v4().to_string();
    let seq = vec![
        make_hello(),
        make_run(&id),
        make_event(&id),
        make_final(&id),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn ref_id_with_special_chars_accepted() {
    let id = "run:123/test-αβγ";
    let seq = vec![make_hello(), make_run(id), make_event(id), make_final(id)];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn multiple_events_with_mixed_ref_ids_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_event("r2"),
        make_event("r1"),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

// ===========================================================================
// 6. Multiple events in sequence
// ===========================================================================

#[test]
fn hundred_events_in_sequence() {
    let mut seq = vec![make_hello(), make_run("r1")];
    for i in 0..100 {
        seq.push(make_event_with_kind(
            "r1",
            AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
        ));
    }
    seq.push(make_final("r1"));
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn mixed_event_types_in_sequence() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event_with_kind(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event_with_kind(
            "r1",
            AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
        ),
        make_event_with_kind(
            "r1",
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls"}),
            },
        ),
        make_event_with_kind(
            "r1",
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("file1\nfile2"),
                is_error: false,
            },
        ),
        make_event_with_kind(
            "r1",
            AgentEventKind::FileChanged {
                path: "main.rs".into(),
                summary: "edited".into(),
            },
        ),
        make_event_with_kind(
            "r1",
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

#[test]
fn events_roundtrip_through_jsonl() {
    let mut seq = vec![make_hello(), make_run("r1")];
    for i in 0..10 {
        seq.push(make_event_with_kind(
            "r1",
            AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            },
        ));
    }
    seq.push(make_final("r1"));
    let jsonl = encode_sequence(&seq);
    let decoded = decode_sequence(&jsonl);
    assert_eq!(decoded.len(), seq.len());
}

#[test]
fn out_of_order_event_before_run_detected() {
    let seq = vec![
        make_hello(),
        make_event("r1"),
        make_run("r1"),
        make_final("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
}

#[test]
fn event_after_final_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_event("r1"),
    ];
    let errors = validator().validate_sequence(&seq);
    // The event after final is out of order, but also we have multiple terminals isn't the issue,
    // the event is after the terminal.
    assert!(!errors.is_empty());
}

#[test]
fn multiple_terminals_detected() {
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_fatal(Some("r1"), "also fail"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

// ===========================================================================
// 7. Large event payloads
// ===========================================================================

#[test]
fn large_assistant_message_roundtrip() {
    let big_text = "x".repeat(1_000_000);
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::AssistantMessage {
            text: big_text.clone(),
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 1_000_000),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn large_tool_output_roundtrip() {
    let big_output = serde_json::json!({"data": "y".repeat(500_000)});
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: big_output.clone(),
            is_error: false,
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert_eq!(output, big_output);
            }
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn large_work_order_context_roundtrip() {
    let mut wo = test_work_order();
    wo.context.snippets = (0..100)
        .map(|i| ContextSnippet {
            name: format!("snippet-{i}"),
            content: "content ".repeat(1000),
        })
        .collect();
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.snippets.len(), 100);
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn large_receipt_trace_roundtrip() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.trace = (0..500)
        .map(|i| AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("delta-{i}"),
            },
            ext: None,
        })
        .collect();
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.trace.len(), 500);
        }
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 8. Unicode in event content
// ===========================================================================

#[test]
fn unicode_in_assistant_message() {
    let text = "日本語テスト 🎉 مرحبا Ñoño café";
    let event = make_event_with_kind("r1", AgentEventKind::AssistantMessage { text: text.into() });
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn unicode_in_tool_call_input() {
    let input = serde_json::json!({"query": "搜索 αβγ δεζ"});
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: input.clone(),
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                input: decoded_input,
                ..
            } => assert_eq!(decoded_input, input),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn unicode_in_backend_identity() {
    let identity = BackendIdentity {
        id: "sidecar:テスト".into(),
        backend_version: Some("1.0-α".into()),
        adapter_version: Some("аdapter-版本".into()),
    };
    let hello = Envelope::hello(identity, test_capabilities());
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "sidecar:テスト");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0-α"));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn unicode_in_fatal_error_message() {
    let fatal = make_fatal(Some("r1"), "Ошибка: 致命的なエラー 💀");
    let rt = round_trip(&fatal);
    match rt {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "Ошибка: 致命的なエラー 💀");
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn unicode_in_file_changed_path() {
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/données/fichier.rs".into(),
            summary: "modifié".into(),
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/données/fichier.rs");
                assert_eq!(summary, "modifié");
            }
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn emoji_heavy_content_roundtrip() {
    let text = "🚀🔥💯🎯🏆🌟✨🎉🎊👋👍🤖💻🔧⚡️🌈";
    let event = make_event_with_kind("r1", AgentEventKind::AssistantDelta { text: text.into() });
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text: t } => assert_eq!(t, text),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn null_bytes_in_content_handled() {
    let text = "before\0after";
    let event = make_event_with_kind("r1", AgentEventKind::AssistantMessage { text: text.into() });
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 9. Empty events
// ===========================================================================

#[test]
fn empty_assistant_delta() {
    let event = make_event_with_kind("r1", AgentEventKind::AssistantDelta { text: "".into() });
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert!(text.is_empty()),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_assistant_message() {
    let event = make_event_with_kind("r1", AgentEventKind::AssistantMessage { text: "".into() });
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert!(text.is_empty()),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_tool_call_input() {
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "noop".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input, serde_json::json!({}));
            }
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_tool_result_output() {
    let event = make_event_with_kind(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "noop".into(),
            tool_use_id: None,
            output: serde_json::Value::Null,
            is_error: false,
        },
    );
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert!(output.is_null());
            }
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_warning_message() {
    let event = make_event_with_kind("r1", AgentEventKind::Warning { message: "".into() });
    let rt = round_trip(&event);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert!(message.is_empty()),
            _ => panic!("wrong kind"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_capabilities_manifest() {
    let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn empty_trace_in_receipt() {
    let receipt = test_receipt(Uuid::nil());
    assert!(receipt.trace.is_empty());
    let f = make_final("r1");
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => assert!(receipt.trace.is_empty()),
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 10. Concurrent sidecar simulation
// ===========================================================================

#[test]
fn independent_sidecar_sequences_validate_separately() {
    let seq1 = vec![
        make_hello(),
        make_run("s1-run"),
        make_event("s1-run"),
        make_final("s1-run"),
    ];
    let seq2 = vec![
        make_hello(),
        make_run("s2-run"),
        make_event("s2-run"),
        make_event("s2-run"),
        make_final("s2-run"),
    ];
    assert!(validator().validate_sequence(&seq1).is_empty());
    assert!(validator().validate_sequence(&seq2).is_empty());
}

#[test]
fn ten_concurrent_sidecar_sequences() {
    for i in 0..10 {
        let id = format!("sidecar-{i}-run");
        let seq = vec![
            Envelope::hello(
                test_identity_with_id(&format!("sidecar-{i}")),
                test_capabilities(),
            ),
            make_run(&id),
            make_event(&id),
            make_event(&id),
            make_final(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(errors.is_empty(), "sidecar {i} failed: {errors:?}");
    }
}

#[test]
fn concurrent_sequences_with_different_capabilities() {
    let caps = vec![
        {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::Streaming, SupportLevel::Native);
            m
        },
        {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::ToolRead, SupportLevel::Native);
            m.insert(Capability::ToolWrite, SupportLevel::Emulated);
            m
        },
        CapabilityManifest::new(),
    ];
    for (i, cap) in caps.iter().enumerate() {
        let id = format!("run-{i}");
        let hello = Envelope::hello(test_identity_with_id(&format!("backend-{i}")), cap.clone());
        let seq = vec![hello, make_run(&id), make_event(&id), make_final(&id)];
        assert!(validator().validate_sequence(&seq).is_empty());
    }
}

#[test]
fn concurrent_encode_decode_isolation() {
    let sequences: Vec<Vec<Envelope>> = (0..5)
        .map(|i| {
            let id = format!("r{i}");
            vec![
                make_hello(),
                make_run(&id),
                make_event(&id),
                make_final(&id),
            ]
        })
        .collect();

    let encoded: Vec<String> = sequences.iter().map(|s| encode_sequence(s)).collect();

    for (i, jsonl) in encoded.iter().enumerate() {
        let decoded = decode_sequence(jsonl);
        assert_eq!(decoded.len(), 4);
        match &decoded[1] {
            Envelope::Run { id, .. } => assert_eq!(id, &format!("r{i}")),
            _ => panic!("expected Run"),
        }
    }
}

// ===========================================================================
// 11. Timeout handling
// ===========================================================================

#[test]
fn host_error_timeout_displays_duration() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = format!("{err}");
    assert!(msg.contains("30"));
}

#[test]
fn host_error_timeout_different_durations() {
    for secs in [1, 5, 10, 30, 60, 300] {
        let err = HostError::Timeout {
            duration: Duration::from_secs(secs),
        };
        let msg = format!("{err}");
        assert!(msg.contains("timed out"));
    }
}

#[test]
fn host_error_exited_with_code() {
    let err = HostError::Exited { code: Some(1) };
    let msg = format!("{err}");
    assert!(msg.contains("1"));
}

#[test]
fn host_error_exited_without_code() {
    let err = HostError::Exited { code: None };
    let msg = format!("{err}");
    assert!(msg.contains("None") || msg.contains("unexpectedly"));
}

#[test]
fn host_error_fatal_message() {
    let err = HostError::Fatal("something went wrong".into());
    let msg = format!("{err}");
    assert!(msg.contains("something went wrong"));
}

#[test]
fn host_error_violation_message() {
    let err = HostError::Violation("protocol mismatch".into());
    let msg = format!("{err}");
    assert!(msg.contains("protocol mismatch"));
}

#[test]
fn host_error_sidecar_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by OOM".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("137") || msg.contains("OOM") || msg.contains("crashed"));
}

// ===========================================================================
// 12. Sidecar process lifecycle (spec → hello → run → cleanup)
// ===========================================================================

#[test]
fn sidecar_spec_new_creates_minimal_spec() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_serde_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "VAL".into());
    let spec = SidecarSpec {
        command: "python".into(),
        args: vec!["-u".into(), "host.py".into()],
        env,
        cwd: Some("/workspace".into()),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let decoded: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, "python");
    assert_eq!(decoded.args.len(), 2);
    assert_eq!(decoded.env["KEY"], "VAL");
    assert_eq!(decoded.cwd.as_deref(), Some("/workspace"));
}

#[test]
fn sidecar_spec_clone_preserves_all_fields() {
    let spec = SidecarSpec {
        command: "node".into(),
        args: vec!["--inspect".into()],
        env: {
            let mut e = BTreeMap::new();
            e.insert("A".into(), "B".into());
            e
        },
        cwd: Some("/dir".into()),
    };
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
    assert_eq!(cloned.args, spec.args);
    assert_eq!(cloned.env, spec.env);
    assert_eq!(cloned.cwd, spec.cwd);
}

#[test]
fn sidecar_hello_captures_identity() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_identity(),
        capabilities: test_capabilities(),
    };
    assert_eq!(hello.backend.id, "test-sidecar");
    assert_eq!(hello.contract_version, CONTRACT_VERSION);
}

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_identity(),
        capabilities: rich_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.backend.id, hello.backend.id);
    assert_eq!(decoded.capabilities.len(), hello.capabilities.len());
}

#[test]
fn sidecar_hello_empty_capabilities() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
    };
    assert!(hello.capabilities.is_empty());
}

// ===========================================================================
// 13. Backend identity from hello
// ===========================================================================

#[test]
fn backend_identity_from_hello_envelope() {
    let hello = make_hello();
    match hello {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("0.1.0"));
            assert!(backend.adapter_version.is_none());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn backend_identity_with_all_fields() {
    let identity = BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("3.5.0".into()),
        adapter_version: Some("1.0.0".into()),
    };
    let hello = Envelope::hello(identity, test_capabilities());
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "sidecar:claude");
            assert_eq!(backend.backend_version.as_deref(), Some("3.5.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.0.0"));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn backend_identity_with_no_version_info() {
    let identity = BackendIdentity {
        id: "minimal-backend".into(),
        backend_version: None,
        adapter_version: None,
    };
    let hello = Envelope::hello(identity, CapabilityManifest::new());
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "minimal-backend");
            assert!(backend.backend_version.is_none());
            assert!(backend.adapter_version.is_none());
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn backend_identity_preserved_in_receipt() {
    let receipt = test_receipt(Uuid::nil());
    assert_eq!(receipt.backend.id, "test-sidecar");
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.backend.id, "test-sidecar");
        }
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 14. Capability extraction from hello
// ===========================================================================

#[test]
fn capabilities_extracted_from_hello() {
    let hello = make_hello();
    match hello {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert_eq!(capabilities.len(), 1);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn rich_capabilities_roundtrip() {
    let caps = rich_capabilities();
    let hello = Envelope::hello(test_identity(), caps.clone());
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 5);
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert!(capabilities.contains_key(&Capability::ToolRead));
            assert!(capabilities.contains_key(&Capability::ToolWrite));
            assert!(capabilities.contains_key(&Capability::ToolEdit));
            assert!(capabilities.contains_key(&Capability::ToolBash));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn capability_support_levels_preserved() {
    let caps = rich_capabilities();
    let hello = Envelope::hello(test_identity(), caps);
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolEdit),
                Some(SupportLevel::Emulated)
            ));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn restricted_capability_roundtrip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let hello = Envelope::hello(test_identity(), caps);
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => match capabilities.get(&Capability::ToolBash) {
            Some(SupportLevel::Restricted { reason }) => {
                assert_eq!(reason, "sandboxed");
            }
            other => panic!("expected Restricted, got {other:?}"),
        },
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn unsupported_capability_roundtrip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);
    let hello = Envelope::hello(test_identity(), caps);
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert!(matches!(
                capabilities.get(&Capability::McpClient),
                Some(SupportLevel::Unsupported)
            ));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn capabilities_in_receipt_match_hello() {
    let caps = rich_capabilities();
    let mut receipt = test_receipt(Uuid::nil());
    receipt.capabilities = caps.clone();
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.capabilities.len(), caps.len());
        }
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 15. Version compatibility and negotiation
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[test]
fn incompatible_with_invalid_versions() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
}

#[test]
fn contract_version_is_current() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

// ===========================================================================
// 16. Execution mode handling
// ===========================================================================

#[test]
fn default_execution_mode_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn hello_with_passthrough_mode() {
    let hello = make_hello_with_mode(ExecutionMode::Passthrough);
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn hello_with_mapped_mode() {
    let hello = make_hello_with_mode(ExecutionMode::Mapped);
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let hello = make_hello();
    match hello {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 17. JSONL codec edge cases
// ===========================================================================

#[test]
fn encode_produces_newline_terminated_string() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encode_contains_tag_field() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains("\"t\":\"hello\""));
}

#[test]
fn encode_run_contains_tag() {
    let run = make_run("r1");
    let encoded = JsonlCodec::encode(&run).unwrap();
    assert!(encoded.contains("\"t\":\"run\""));
}

#[test]
fn encode_event_contains_tag() {
    let event = make_event("r1");
    let encoded = JsonlCodec::encode(&event).unwrap();
    assert!(encoded.contains("\"t\":\"event\""));
}

#[test]
fn encode_final_contains_tag() {
    let f = make_final("r1");
    let encoded = JsonlCodec::encode(&f).unwrap();
    assert!(encoded.contains("\"t\":\"final\""));
}

#[test]
fn encode_fatal_contains_tag() {
    let fatal = make_fatal(Some("r1"), "boom");
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    assert!(encoded.contains("\"t\":\"fatal\""));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n";
    let reader = BufReader::new(Cursor::new(input));
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn decode_stream_multiple_envelopes() {
    let seq = vec![make_hello(), make_run("r1"), make_final("r1")];
    let jsonl = encode_sequence(&seq);
    let decoded = decode_sequence(&jsonl);
    assert_eq!(decoded.len(), 3);
}

#[test]
fn encode_to_writer_works() {
    let hello = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("\"t\":\"hello\""));
}

#[test]
fn encode_many_to_writer_preserves_order() {
    let envelopes = vec![make_hello(), make_run("r1"), make_final("r1")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let hello_pos = s.find("\"t\":\"hello\"").unwrap();
    let run_pos = s.find("\"t\":\"run\"").unwrap();
    let final_pos = s.find("\"t\":\"final\"").unwrap();
    assert!(hello_pos < run_pos);
    assert!(run_pos < final_pos);
}

// ===========================================================================
// 18. Protocol error handling
// ===========================================================================

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    let msg = format!("{err}");
    assert!(msg.contains("JSON") || msg.contains("json"));
}

#[test]
fn protocol_error_violation_carries_error_code() {
    let err = ProtocolError::Violation("bad message".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_carries_error_code() {
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
fn protocol_error_display_formatting() {
    let err = ProtocolError::Violation("test violation".into());
    let msg = format!("{err}");
    assert!(msg.contains("test violation"));
}

// ===========================================================================
// 19. Receipt hashing in lifecycle context
// ===========================================================================

#[test]
fn receipt_hash_is_deterministic() {
    let receipt = test_receipt(Uuid::nil());
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn receipt_with_hash_fills_field() {
    let receipt = test_receipt(Uuid::nil());
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_in_final_envelope_roundtrips_with_hash() {
    let hashed = test_receipt(Uuid::nil()).with_hash().unwrap();
    let sha = hashed.receipt_sha256.clone();
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt: hashed,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.receipt_sha256, sha);
        }
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 20. Sequence validation edge cases
// ===========================================================================

#[test]
fn missing_terminal_detected() {
    let seq = vec![make_hello(), make_run("r1"), make_event("r1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn only_hello_missing_terminal() {
    let seq = vec![make_hello()];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn hello_and_run_only_missing_terminal() {
    let seq = vec![make_hello(), make_run("r1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn fatal_as_abp_error() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendCrashed, "process died");
    let fatal = Envelope::fatal_from_abp_error(Some("r1".into()), &abp_err);
    match &fatal {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "process died");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendCrashed));
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 21. Agent event extension field
// ===========================================================================

#[test]
fn event_with_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"vendor": "data"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        ext: Some(ext),
    };
    let envelope = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let rt = round_trip(&envelope);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn event_without_ext_field_is_none() {
    let event = test_event();
    assert!(event.ext.is_none());
    let envelope = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let rt = round_trip(&envelope);
    match rt {
        Envelope::Event { event, .. } => assert!(event.ext.is_none()),
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 22. Outcome variants in receipt
// ===========================================================================

#[test]
fn receipt_outcome_complete() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Complete;
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Complete),
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn receipt_outcome_partial() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Partial;
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn receipt_outcome_failed() {
    let mut receipt = test_receipt(Uuid::nil());
    receipt.outcome = Outcome::Failed;
    let f = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = round_trip(&f);
    match rt {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        _ => panic!("wrong envelope"),
    }
}

// ===========================================================================
// 23. Work order in Run envelope
// ===========================================================================

#[test]
fn work_order_task_preserved_in_run() {
    let run = make_run("r1");
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn work_order_with_policy_roundtrip() {
    let mut wo = test_work_order();
    wo.policy.allowed_tools = vec!["read".into(), "write".into()];
    wo.policy.deny_read = vec!["*.secret".into()];
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.policy.allowed_tools.len(), 2);
            assert_eq!(work_order.policy.deny_read.len(), 1);
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn work_order_with_config_roundtrip() {
    let mut wo = test_work_order();
    wo.config.model = Some("claude-3".into());
    wo.config.max_turns = Some(10);
    wo.config.max_budget_usd = Some(5.0);
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("claude-3"));
            assert_eq!(work_order.config.max_turns, Some(10));
        }
        _ => panic!("wrong envelope"),
    }
}

#[test]
fn work_order_workspace_first_lane() {
    let mut wo = test_work_order();
    wo.lane = ExecutionLane::WorkspaceFirst;
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: wo,
    };
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.lane, ExecutionLane::WorkspaceFirst);
        }
        _ => panic!("wrong envelope"),
    }
}
