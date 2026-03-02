// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol-level unit tests for abp-host.
//!
//! These tests exercise JSONL parsing, Envelope deserialization, validation,
//! and protocol state machine logic using pure Rust code paths (no process spawning).

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, CapabilityRequirements,
    ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt, RunMetadata,
    RuntimeConfig, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use std::io::BufReader;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_receipt(run_id: Uuid, wo_id: Uuid) -> Receipt {
    let now = chrono::Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo_id,
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
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

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
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

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. JSONL line parsing: valid JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_valid_hello_json() {
    let line = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"test","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let envelope = JsonlCodec::decode(&line).unwrap();
    assert!(matches!(envelope, Envelope::Hello { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. JSONL line parsing: invalid JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn decode_truncated_json_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. JSONL line parsing: empty line
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_empty_string_returns_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. JSONL line parsing: whitespace-only line
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_whitespace_only_returns_error() {
    let result = JsonlCodec::decode("   \t  ");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Envelope deserialization: Hello variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_roundtrip() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
    assert!(encoded.contains(r#""t":"hello""#));

    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            mode,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-backend");
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn envelope_hello_with_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Envelope deserialization: Run variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_run_roundtrip() {
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-42".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    assert!(encoded.contains(r#""t":"run""#));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-42");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Envelope deserialization: Event variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_event_roundtrip() {
    let event_env = make_event(
        "run-42",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let encoded = JsonlCodec::encode(&event_env).unwrap();
    assert!(encoded.contains(r#""t":"event""#));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-42");
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Envelope deserialization: Final variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_final_roundtrip() {
    let run_id = Uuid::new_v4();
    let wo_id = Uuid::nil();
    let final_env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt: test_receipt(run_id, wo_id),
    };
    let encoded = JsonlCodec::encode(&final_env).unwrap();
    assert!(encoded.contains(r#""t":"final""#));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Envelope deserialization: Fatal variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_fatal_with_ref_id_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    assert!(encoded.contains(r#""t":"fatal""#));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-42".into()));
            assert_eq!(error, "out of memory");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_fatal_without_ref_id_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "global failure".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global failure");
        }
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Hello validation: required fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_hello_with_empty_backend_id() {
    let hello = Envelope::Hello {
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
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn validate_hello_with_invalid_version() {
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

#[test]
fn validate_hello_warns_on_missing_optional_fields() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. ref_id correlation validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_event_with_empty_ref_id() {
    let event_env = make_event(
        "",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&event_env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "ref_id"
    )));
}

#[test]
fn validate_final_with_empty_ref_id() {
    let final_env = Envelope::Final {
        ref_id: String::new(),
        receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&final_env);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Protocol state machine: valid sequence hello→run→event*→final
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_sequence_hello_run_events_final() {
    let run_id = "run-1";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(run_id, AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors, got: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Invalid state transition: event before hello
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_missing_hello() {
    let sequence = vec![
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingHello)));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Invalid state transition: hello not first
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_hello_not_first() {
    let run_id = "run-1";
    let sequence = vec![
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { position: 1 })));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Invalid state transition: missing terminal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_missing_terminal() {
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: "run-1".into(),
            work_order: test_work_order(),
        },
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. ref_id mismatch in sequence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_ref_id_mismatch_detected() {
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: "run-1".into(),
            work_order: test_work_order(),
        },
        make_event(
            "run-WRONG",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
            if expected == "run-1" && found == "run-WRONG"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Fatal envelope handling in sequence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_sequence_with_fatal_instead_of_final() {
    let run_id = "run-1";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Fatal {
            ref_id: Some(run_id.into()),
            error: "catastrophic failure".into(),
            error_code: None,
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors.is_empty(),
        "fatal as terminal should be valid, got: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Large payload handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_payload_event_round_trips() {
    let big_text = "a".repeat(200_000);
    let event_env = Envelope::Event {
        ref_id: "run-big".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: big_text.clone(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert_eq!(text.len(), 200_000);
            } else {
                panic!("expected AssistantMessage");
            }
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Unicode in JSONL lines
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unicode_in_envelope_fields() {
    let event_env = Envelope::Event {
        ref_id: "実行-42".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "こんにちは世界 🌍 Ω ñ é ü".into(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "実行-42");
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert!(text.contains("こんにちは"));
                assert!(text.contains("🌍"));
            } else {
                panic!("expected AssistantMessage");
            }
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Multiple envelopes concatenated (decode_stream)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_stream_multiple_envelopes() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let line1 = JsonlCodec::encode(&hello).unwrap();
    let line2 = JsonlCodec::encode(&fatal).unwrap();
    let input = format!("{line1}{line2}");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let input = format!("\n\n{line}\n\n{line}\n");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Protocol tag field is "t" not "type"
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_tag_is_t_not_type() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let json_str = serde_json::to_string(&hello).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["t"], "hello");
    assert!(v.get("type").is_none() || v["type"] != "hello");
}

#[test]
fn fatal_tag_is_t() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let v: serde_json::Value = serde_json::to_value(&fatal).unwrap();
    assert_eq!(v["t"], "fatal");
}

#[test]
fn event_tag_is_t() {
    let event_env = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let v: serde_json::Value = serde_json::to_value(&event_env).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn final_tag_is_t() {
    let final_env = Envelope::Final {
        ref_id: "r1".into(),
        receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
    };
    let v: serde_json::Value = serde_json::to_value(&final_env).unwrap();
    assert_eq!(v["t"], "final");
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Decode from raw JSON string (simulating sidecar output)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_raw_hello_json_string() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"mock","backend_version":"0.1","adapter_version":null},"capabilities":{}}"#;
    let envelope = JsonlCodec::decode(raw).unwrap();
    match envelope {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "mock");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn decode_unknown_tag_returns_error() {
    let raw = r#"{"t":"unknown_type","data":"something"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
}

#[test]
fn decode_missing_tag_returns_error() {
    let raw = r#"{"contract_version":"abp/v0.1","backend":{"id":"mock"}}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 23. encode_to_writer and encode_many_to_writer
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_to_writer_produces_valid_jsonl() {
    let mut buf = Vec::new();
    let env = Envelope::Fatal {
        ref_id: None,
        error: "oops".into(),
        error_code: None,
    };
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    let decoded = JsonlCodec::decode(s.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn encode_many_to_writer_produces_multiple_lines() {
    let mut buf = Vec::new();
    let envelopes = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        },
    ];
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Sequence validation: empty sequence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_sequence_has_errors() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingHello)));
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Sequence validation: multiple terminals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_multiple_terminals_detected() {
    let run_id = "run-1";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
        Envelope::Fatal {
            ref_id: Some(run_id.into()),
            error: "extra".into(),
            error_code: None,
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MultipleTerminals)));
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Event out-of-order: event before run
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_event_before_run_is_out_of_order() {
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Run {
            id: "run-1".into(),
            work_order: test_work_order(),
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Version parsing and compatibility
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(abp_protocol::parse_version("not-a-version"), None);
    assert_eq!(abp_protocol::parse_version("abp/v"), None);
    assert_eq!(abp_protocol::parse_version(""), None);
}

#[test]
fn is_compatible_version_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn is_compatible_version_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!abp_protocol::is_compatible_version("abp/v2.0", "abp/v1.0"));
}

// ═══════════════════════════════════════════════════════════════════════
// 28. Validate Run with empty fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_run_with_empty_id() {
    let run = Envelope::Run {
        id: String::new(),
        work_order: test_work_order(),
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&run);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "id"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Validate Fatal with empty error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_fatal_with_empty_error() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
        error_code: None,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&fatal);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "error"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 30. Valid envelope passes validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_hello_passes_validation() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}
