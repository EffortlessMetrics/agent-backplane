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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep conformance harness tests for the ABP sidecar protocol.
//!
//! Validates:
//! 1. Hello handshake validation
//! 2. Envelope ordering (hello → run → event* → final/fatal)
//! 3. ref_id correlation
//! 4. Event streaming (various AgentEventKind variants)
//! 5. Fatal handling
//! 6. Final receipt validation
//! 7. Contract version negotiation
//! 8. Capability reporting
//! 9. JSONL line discipline
//! 10. Edge cases

use abp_core::*;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::version::{ProtocolVersion, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "deep-conformance".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn make_hello_with_version(version: &str) -> Envelope {
    Envelope::Hello {
        contract_version: version.to_string(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::Mapped,
    }
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: WorkOrderBuilder::new("conformance test task").build(),
    }
}

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let now = Utc::now();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: Receipt {
            meta: RunMetadata {
                run_id: Uuid::new_v4(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: now,
                finished_at: now,
                duration_ms: 10,
            },
            backend: test_backend(),
            capabilities: test_capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        },
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn encode_stream(envelopes: &[Envelope]) -> Vec<u8> {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, envelopes).unwrap();
    buf
}

fn decode_all(buf: &[u8]) -> Vec<Result<Envelope, ProtocolError>> {
    let reader = BufReader::new(buf);
    JsonlCodec::decode_stream(reader).collect()
}

fn decode_all_ok(buf: &[u8]) -> Vec<Envelope> {
    decode_all(buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

// ═════════════════════════════════════════════════════════════════════════
// 1. Hello handshake validation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn hello_must_be_first_envelope_in_stream() {
    let envelopes = vec![make_hello(), make_run("r1"), make_final("r1")];
    let buf = encode_stream(&envelopes);
    let decoded = decode_all_ok(&buf);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
}

#[test]
fn hello_contains_contract_version() {
    let hello = make_hello();
    if let Envelope::Hello {
        contract_version, ..
    } = &hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_contains_backend_identity() {
    let hello = make_hello();
    if let Envelope::Hello { backend, .. } = &hello {
        assert!(!backend.id.is_empty());
        assert!(backend.backend_version.is_some());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_contains_capabilities() {
    let hello = make_hello();
    if let Envelope::Hello { capabilities, .. } = &hello {
        assert!(!capabilities.is_empty());
        assert!(capabilities.contains_key(&Capability::Streaming));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_with_empty_capabilities_is_valid() {
    let hello = Envelope::hello(test_backend(), BTreeMap::new());
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(result.valid);
}

#[test]
fn hello_with_empty_backend_id_is_invalid() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
}

#[test]
fn hello_with_empty_contract_version_is_invalid() {
    let hello = Envelope::Hello {
        contract_version: String::new(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::Mapped,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
}

#[test]
fn hello_with_invalid_version_format_is_invalid() {
    let hello = make_hello_with_version("not-a-version");
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
}

#[test]
fn hello_roundtrips_through_jsonl() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

// ═════════════════════════════════════════════════════════════════════════
// 2. Envelope ordering
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn valid_ordering_hello_run_events_final() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event("r1", AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}

#[test]
fn valid_ordering_hello_run_fatal() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_fatal(Some("r1"), "something went wrong"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(errors.is_empty(), "hello→run→fatal is valid: {errors:?}");
}

#[test]
fn invalid_ordering_missing_hello() {
    let envelopes = vec![make_run("r1"), make_final("r1")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn invalid_ordering_hello_not_first() {
    let envelopes = vec![make_run("r1"), make_hello(), make_final("r1")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "hello at position 1 should be flagged"
    );
}

#[test]
fn invalid_ordering_event_before_run() {
    let envelopes = vec![
        make_hello(),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "premature".into(),
            },
        ),
        make_run("r1"),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "event before run should be flagged"
    );
}

#[test]
fn invalid_ordering_event_after_final() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_event(
            "r1",
            AgentEventKind::RunCompleted {
                message: "late".into(),
            },
        ),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "event after final should be flagged"
    );
}

#[test]
fn invalid_ordering_multiple_terminals() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_final("r1"),
        make_fatal(Some("r1"), "extra"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn invalid_ordering_missing_terminal() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn empty_sequence_flags_missing_hello_and_terminal() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

// ═════════════════════════════════════════════════════════════════════════
// 3. ref_id correlation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn event_ref_id_must_match_run_id() {
    let envelopes = vec![
        make_hello(),
        make_run("run-42"),
        make_event(
            "run-42",
            AgentEventKind::RunStarted {
                message: "ok".into(),
            },
        ),
        make_final("run-42"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(errors.is_empty());
}

#[test]
fn mismatched_event_ref_id_is_flagged() {
    let envelopes = vec![
        make_hello(),
        make_run("run-42"),
        make_event(
            "wrong-id",
            AgentEventKind::RunStarted {
                message: "oops".into(),
            },
        ),
        make_final("run-42"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched event ref_id should be flagged"
    );
}

#[test]
fn mismatched_final_ref_id_is_flagged() {
    let envelopes = vec![make_hello(), make_run("run-42"), make_final("wrong-id")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched final ref_id should be flagged"
    );
}

#[test]
fn mismatched_fatal_ref_id_is_flagged() {
    let envelopes = vec![
        make_hello(),
        make_run("run-42"),
        make_fatal(Some("wrong-id"), "boom"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched fatal ref_id should be flagged"
    );
}

#[test]
fn fatal_with_none_ref_id_does_not_flag_mismatch() {
    let envelopes = vec![
        make_hello(),
        make_run("run-42"),
        make_fatal(None, "crash before run started"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        !errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "fatal with None ref_id should not flag mismatch"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 4. Event streaming — various AgentEventKind variants
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn event_text_delta_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "hello world".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantDelta { ref text } if text == "hello world"
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(
            matches!(event.kind, AgentEventKind::ToolCall { ref tool_name, .. } if tool_name == "read_file")
        );
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::ToolResult {
                is_error: false,
                ..
            }
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::Error {
            message: "something failed".into(),
            error_code: None,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::Error { ref message, .. } if message == "something failed"
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error_with_error_code_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::Error {
            message: "tool failed".into(),
            error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Error { error_code, .. } = &event.kind {
            assert!(error_code.is_some());
        } else {
            panic!("expected Error kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_run_started_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_run_completed_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_message_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "full response text".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { ref text } if text == "full response text"
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_file_changed_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added function".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::FileChanged { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::CommandExecuted {
                exit_code: Some(0),
                ..
            }
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_warning_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::Warning {
            message: "rate limit approaching".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn multi_event_sequence_validates() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event("r1", AgentEventKind::AssistantDelta { text: "hel".into() }),
        make_event("r1", AgentEventKind::AssistantDelta { text: "lo".into() }),
        make_event(
            "r1",
            AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls"}),
            },
        ),
        make_event(
            "r1",
            AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("file1\nfile2"),
                is_error: false,
            },
        ),
        make_event(
            "r1",
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final("r1"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.is_empty(),
        "full event sequence should be valid: {errors:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 5. Fatal handling
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_roundtrips() {
    let fatal = make_fatal(Some("r1"), "out of memory");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert_eq!(ref_id.as_deref(), Some("r1"));
        assert_eq!(error, "out of memory");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_error_code_roundtrips() {
    let fatal = Envelope::fatal_with_code(
        Some("r1".into()),
        "backend crashed",
        abp_error::ErrorCode::BackendCrashed,
    );
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(decoded.error_code().is_some());
}

#[test]
fn fatal_with_none_ref_id_roundtrips() {
    let fatal = make_fatal(None, "unknown error before run");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { ref_id, .. } = decoded {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_empty_error_is_invalid() {
    let fatal = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: String::new(),
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&fatal);
    assert!(!result.valid);
}

#[test]
fn fatal_terminates_valid_sequence() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_fatal(Some("r1"), "crash"),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&envelopes);
    assert!(
        errors.is_empty(),
        "hello→run→event→fatal is valid: {errors:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 6. Final receipt
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_contains_receipt() {
    let fin = make_final("r1");
    if let Envelope::Final { receipt, .. } = &fin {
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert_eq!(receipt.outcome, Outcome::Complete);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_roundtrips_through_jsonl() {
    let fin = make_final("r1");
    let line = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, ref_id } = decoded {
        assert_eq!(ref_id, "r1");
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert_eq!(receipt.outcome, Outcome::Complete);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_has_required_metadata_fields() {
    let fin = make_final("r1");
    if let Envelope::Final { receipt, .. } = &fin {
        assert!(!receipt.meta.contract_version.is_empty());
        assert!(!receipt.backend.id.is_empty());
        assert!(receipt.meta.duration_ms <= 1_000_000);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_hash_is_valid() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn final_receipt_hash_is_deterministic() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();

    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn final_receipt_with_empty_ref_id_is_invalid() {
    let fin = Envelope::Final {
        ref_id: String::new(),
        receipt: ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .build(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&fin);
    assert!(!result.valid);
}

#[test]
fn final_receipt_with_trace_events() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(event)
        .build();

    assert_eq!(receipt.trace.len(), 1);

    let fin = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.trace.len(), 1);
    } else {
        panic!("expected Final");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 7. Contract version negotiation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    let v = parse_version(CONTRACT_VERSION);
    assert_eq!(v, Some((0, 1)));
}

#[test]
fn parse_version_higher_minor() {
    let v = parse_version("abp/v0.2");
    assert_eq!(v, Some((0, 2)));
}

#[test]
fn parse_version_invalid_returns_none() {
    assert!(parse_version("invalid").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("abp/0.1").is_none());
    assert!(parse_version("").is_none());
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn protocol_version_negotiate_same_major() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.major, 0);
    assert_eq!(result.minor, 1);
}

#[test]
fn protocol_version_negotiate_different_major_fails() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&local, &remote).is_err());
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

// ═════════════════════════════════════════════════════════════════════════
// 8. Capability reporting
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn capabilities_in_hello_are_preserved_through_roundtrip() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let hello = Envelope::hello(test_backend(), caps);
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), 3);
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert!(capabilities.contains_key(&Capability::ToolRead));
        assert!(capabilities.contains_key(&Capability::ToolWrite));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn capabilities_support_levels_roundtrip() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    );

    let hello = Envelope::hello(test_backend(), caps);
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(matches!(
            capabilities.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolBash),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            capabilities.get(&Capability::McpClient),
            Some(SupportLevel::Restricted { .. })
        ));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn capability_satisfies_minimum_support() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ═════════════════════════════════════════════════════════════════════════
// 9. JSONL line discipline
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn encoded_envelope_is_single_line_terminated_by_newline() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'), "must end with newline");
    let trimmed = line.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "JSON must be on a single line (no embedded newlines)"
    );
}

#[test]
fn all_envelope_types_are_single_line() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final("r1"),
        make_fatal(Some("r1"), "error"),
    ];
    for env in &envelopes {
        let line = JsonlCodec::encode(env).unwrap();
        assert!(line.ends_with('\n'));
        let trimmed = line.trim_end_matches('\n');
        assert!(
            !trimmed.contains('\n'),
            "embedded newline in envelope: {trimmed}"
        );
    }
}

#[test]
fn jsonl_stream_has_one_envelope_per_line() {
    let envelopes = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1", AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_final("r1"),
    ];
    let buf = encode_stream(&envelopes);
    let text = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "4 envelopes = 4 lines");

    for line in &lines {
        let decoded = JsonlCodec::decode(line);
        assert!(decoded.is_ok(), "each line should decode: {line}");
    }
}

#[test]
fn blank_lines_in_stream_are_skipped() {
    let hello_json = JsonlCodec::encode(&make_hello()).unwrap();
    let final_json = JsonlCodec::encode(&make_final("r1")).unwrap();
    let stream = format!("{hello_json}\n\n{final_json}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn envelope_tag_field_is_t_not_type() {
    let hello = make_hello();
    let json = serde_json::to_value(&hello).unwrap();
    assert!(json.get("t").is_some(), "discriminator field should be 't'");
    assert!(
        json.get("type").is_none(),
        "should not have 'type' at envelope level"
    );
}

#[test]
fn event_kind_tag_field_is_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(
        json.get("type").is_some(),
        "AgentEventKind discriminator should be 'type'"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 10. Edge cases
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn empty_text_delta_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantDelta {
            text: String::new(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantDelta { text } = &event.kind {
            assert!(text.is_empty());
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn very_long_content_roundtrips() {
    let long_text = "a".repeat(100_000);
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: long_text.clone(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "long content must still be single line"
    );
    let decoded = JsonlCodec::decode(trimmed).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text.len(), 100_000);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn unicode_content_roundtrips() {
    let unicode_text = "Hello 世界! 🌍🚀 Ñoño café résumé наука";
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: unicode_text.into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, unicode_text);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn special_json_characters_in_content_roundtrip() {
    let special = "line1\nline2\ttab\"quote\\backslash";
    let env = make_event(
        "r1",
        AgentEventKind::AssistantMessage {
            text: special.into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "escaped newlines must not break line discipline"
    );
    let decoded = JsonlCodec::decode(trimmed).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, special);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn invalid_json_line_returns_error() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
}

#[test]
fn unknown_envelope_type_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":123}"#);
    assert!(result.is_err());
}

#[test]
fn run_envelope_with_empty_id_is_invalid() {
    let run = Envelope::Run {
        id: String::new(),
        work_order: WorkOrderBuilder::new("test").build(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&run);
    assert!(!result.valid);
}

#[test]
fn run_envelope_with_empty_task_is_invalid() {
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: WorkOrderBuilder::new("").build(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&run);
    assert!(!result.valid);
}

#[test]
fn event_with_empty_ref_id_is_invalid() {
    let env = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn execution_mode_defaults_to_mapped() {
    let hello = make_hello();
    if let Envelope::Hello { mode, .. } = hello {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn execution_mode_passthrough_roundtrips() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn tool_result_with_error_flag_roundtrips() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"error": "command not found"}),
            is_error: true,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(matches!(
            event.kind,
            AgentEventKind::ToolResult { is_error: true, .. }
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn ext_field_preserves_passthrough_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor": "test", "data": [1, 2, 3]}),
    );
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
    if let Envelope::Event { event, .. } = decoded {
        assert!(event.ext.is_some());
        let ext = event.ext.unwrap();
        assert!(ext.contains_key("raw_message"));
    } else {
        panic!("expected Event");
    }
}
