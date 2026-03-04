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
//! Comprehensive sidecar protocol conformance test suite.
//!
//! Validates the ABP JSONL sidecar protocol specification using mock IO
//! streams — no external processes are spawned. Covers: hello handshake,
//! capabilities, run envelopes, event streaming, final/fatal envelopes,
//! protocol ordering, invalid sequences, malformed JSON, large payloads,
//! contract version negotiation, and edge cases.

use abp_core::*;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError, RawFrame, is_compatible_version, parse_version,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "conformance-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps
}

fn multi_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
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
        mode: ExecutionMode::default(),
    }
}

fn make_run(run_id: &str) -> Envelope {
    Envelope::Run {
        id: run_id.into(),
        work_order: test_work_order(),
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
                run_id: Uuid::nil(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.into(),
                started_at: now,
                finished_at: now,
                duration_ms: 100,
            },
            backend: test_backend(),
            capabilities: test_capabilities(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
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

fn encode(envelope: &Envelope) -> String {
    JsonlCodec::encode(envelope).expect("encode should succeed")
}

fn decode(line: &str) -> Result<Envelope, ProtocolError> {
    JsonlCodec::decode(line.trim())
}

/// Encode a sequence of envelopes into a JSONL string.
fn encode_stream(envelopes: &[Envelope]) -> String {
    envelopes.iter().map(|e| encode(e)).collect::<String>()
}

/// Decode a JSONL string into a vector of envelopes.
fn decode_stream(input: &str) -> Vec<Result<Envelope, ProtocolError>> {
    let reader = BufReader::new(input.as_bytes());
    JsonlCodec::decode_stream(reader).collect()
}

fn validator() -> EnvelopeValidator {
    EnvelopeValidator::new()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Hello Handshake
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_contains_tag_t() {
    let line = encode(&make_hello());
    assert!(
        line.contains(r#""t":"hello""#),
        "envelope tag must be \"t\", not \"type\""
    );
}

#[test]
fn hello_contains_contract_version() {
    let line = encode(&make_hello());
    assert!(line.contains(CONTRACT_VERSION));
}

#[test]
fn hello_contract_version_value() {
    let env = make_hello();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, "abp/v0.1");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_roundtrip() {
    let line = encode(&make_hello());
    let decoded = decode(&line).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_line_ends_with_newline() {
    let line = encode(&make_hello());
    assert!(line.ends_with('\n'));
}

#[test]
fn hello_backend_identity_preserved() {
    let line = encode(&make_hello());
    let decoded = decode(&line).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, "conformance-sidecar");
        assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = make_hello();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_with_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_validates_successfully() {
    let result = validator().validate(&make_hello());
    assert!(
        result.valid,
        "hello envelope should validate: {:?}",
        result.errors
    );
}

#[test]
fn hello_empty_backend_id_fails_validation() {
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
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id"))
    );
}

#[test]
fn hello_empty_contract_version_fails_validation() {
    let env = Envelope::Hello {
        contract_version: String::new(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "contract_version"
    )));
}

#[test]
fn hello_invalid_contract_version_fails_validation() {
    let env = make_hello_with_version("not-a-version");
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::InvalidVersion { version } if version == "not-a-version"
    )));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Hello Capabilities
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_capabilities_empty_is_valid() {
    let env = Envelope::hello(test_backend(), CapabilityManifest::new());
    let result = validator().validate(&env);
    assert!(result.valid);
}

#[test]
fn hello_capabilities_single_entry() {
    let line = encode(&make_hello());
    assert!(line.contains("streaming"));
}

#[test]
fn hello_capabilities_multiple_entries() {
    let env = Envelope::hello(test_backend(), multi_capabilities());
    let line = encode(&env);
    assert!(line.contains("streaming"));
    assert!(line.contains("tool_read"));
    assert!(line.contains("tool_write"));
    assert!(line.contains("tool_bash"));
}

#[test]
fn hello_capabilities_support_levels_preserved() {
    let env = Envelope::hello(test_backend(), multi_capabilities());
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(matches!(
            capabilities.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolWrite),
            Some(SupportLevel::Emulated)
        ));
        assert!(matches!(
            capabilities.get(&Capability::ToolBash),
            Some(SupportLevel::Unsupported)
        ));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_capabilities_roundtrip_deterministic() {
    let env = Envelope::hello(test_backend(), multi_capabilities());
    let line1 = encode(&env);
    let line2 = encode(&env);
    assert_eq!(
        line1, line2,
        "BTreeMap should produce deterministic ordering"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Run Envelope
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_envelope_tag() {
    let line = encode(&make_run("run-1"));
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn run_envelope_roundtrip() {
    let line = encode(&make_run("run-1"));
    let decoded = decode(&line).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-1");
        assert_eq!(work_order.task, "hello world");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_preserves_work_order_id() {
    let line = encode(&make_run("run-1"));
    let decoded = decode(&line).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.id, Uuid::nil());
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_envelope_validates_successfully() {
    let result = validator().validate(&make_run("run-1"));
    assert!(result.valid);
}

#[test]
fn run_envelope_empty_id_fails_validation() {
    let env = Envelope::Run {
        id: String::new(),
        work_order: test_work_order(),
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "id"
    )));
}

#[test]
fn run_envelope_empty_task_fails_validation() {
    let mut wo = test_work_order();
    wo.task = String::new();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Event Streaming
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_envelope_tag() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunStarted {
            message: "started".into(),
        },
    );
    let line = encode(&env);
    assert!(line.contains(r#""t":"event""#));
}

#[test]
fn event_ref_id_preserved() {
    let env = make_event(
        "run-42",
        AgentEventKind::RunStarted {
            message: "hi".into(),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { ref_id, .. } = decoded {
        assert_eq!(ref_id, "run-42");
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_stream_multiple_events_correlated() {
    let run_id = "run-multi";
    let events = vec![
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "token1".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "token2".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
    ];

    let stream = encode_stream(&events);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), 4);

    for result in decoded {
        let env = result.unwrap();
        if let Envelope::Event { ref_id, .. } = env {
            assert_eq!(ref_id, run_id);
        } else {
            panic!("expected Event");
        }
    }
}

#[test]
fn event_validates_successfully() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
    );
    let result = validator().validate(&env);
    assert!(result.valid);
}

#[test]
fn event_empty_ref_id_fails_validation() {
    let env = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "x".into(),
            },
            ext: None,
        },
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "ref_id"
    )));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Final Envelope
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_tag() {
    let line = encode(&make_final("run-1"));
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn final_roundtrip() {
    let line = encode(&make_final("run-1"));
    let decoded = decode(&line).unwrap();
    if let Envelope::Final { ref_id, receipt } = decoded {
        assert_eq!(ref_id, "run-1");
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_validates_successfully() {
    let result = validator().validate(&make_final("run-1"));
    assert!(result.valid);
}

#[test]
fn final_empty_ref_id_fails_validation() {
    let env = Envelope::Final {
        ref_id: String::new(),
        receipt: Receipt {
            meta: RunMetadata {
                run_id: Uuid::nil(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.into(),
                started_at: Utc::now(),
                finished_at: Utc::now(),
                duration_ms: 0,
            },
            backend: test_backend(),
            capabilities: test_capabilities(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        },
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
}

#[test]
fn final_receipt_contract_version_matches() {
    let env = make_final("run-1");
    if let Envelope::Final { receipt, .. } = env {
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Final");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Fatal Envelope
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_tag() {
    let line = encode(&make_fatal(Some("run-1"), "boom"));
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn fatal_roundtrip_with_ref_id() {
    let env = make_fatal(Some("run-1"), "crash");
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = decoded
    {
        assert_eq!(ref_id, Some("run-1".into()));
        assert_eq!(error, "crash");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_without_ref_id() {
    let env = make_fatal(None, "early error");
    let line = encode(&env);
    assert!(line.contains(r#""ref_id":null"#));
    let decoded = decode(&line).unwrap();
    if let Envelope::Fatal { ref_id, .. } = decoded {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "protocol failure",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Fatal { error_code, .. } = decoded {
        assert_eq!(
            error_code,
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_validates_successfully() {
    let result = validator().validate(&make_fatal(Some("run-1"), "error msg"));
    assert!(result.valid);
}

#[test]
fn fatal_empty_error_fails_validation() {
    let env = make_fatal(Some("run-1"), "");
    let result = validator().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "error"
    )));
}

#[test]
fn fatal_missing_ref_id_warns() {
    let env = make_fatal(None, "some error");
    let result = validator().validate(&env);
    assert!(result.valid, "missing ref_id is a warning, not an error");
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "ref_id"
    )));
}

#[test]
fn fatal_error_code_accessor() {
    let env =
        Envelope::fatal_with_code(None, "fail", abp_error::ErrorCode::ProtocolHandshakeFailed);
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
    );
}

#[test]
fn non_fatal_error_code_is_none() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Protocol Ordering (hello → run → events → final/fatal)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_sequence_hello_run_event_final() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final("run-1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_fatal() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_fatal(Some("run-1"), "abort"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_many_events_final() {
    let mut seq = vec![make_hello(), make_run("run-1")];
    for i in 0..10 {
        seq.push(make_event(
            "run-1",
            AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            },
        ));
    }
    seq.push(make_final("run-1"));
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn valid_sequence_hello_run_no_events_final() {
    let seq = vec![make_hello(), make_run("run-1"), make_final("run-1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Invalid Sequences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_sequence_missing_hello() {
    let seq = vec![make_run("run-1"), make_final("run-1")];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn invalid_sequence_hello_not_first() {
    let seq = vec![make_run("run-1"), make_hello(), make_final("run-1")];
    let errors = validator().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn invalid_sequence_missing_terminal() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event(
            "run-1",
            AgentEventKind::AssistantDelta { text: "tok".into() },
        ),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn invalid_sequence_multiple_terminals() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_final("run-1"),
        make_fatal(Some("run-1"), "extra"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn invalid_sequence_empty() {
    let errors = validator().validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn invalid_sequence_event_before_run() {
    let seq = vec![
        make_hello(),
        make_event(
            "run-1",
            AgentEventKind::AssistantMessage {
                text: "early".into(),
            },
        ),
        make_run("run-1"),
        make_final("run-1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn invalid_sequence_event_after_terminal() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_final("run-1"),
        make_event(
            "run-1",
            AgentEventKind::AssistantMessage {
                text: "late".into(),
            },
        ),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(!errors.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Missing ref_id / ref_id Mismatch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_with_mismatched_ref_id() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event(
            "wrong-run",
            AgentEventKind::AssistantDelta {
                text: "oops".into(),
            },
        ),
        make_final("run-1"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
            if expected == "run-1" && found == "wrong-run"
    )));
}

#[test]
fn final_with_mismatched_ref_id() {
    let seq = vec![make_hello(), make_run("run-1"), make_final("wrong-run")];
    let errors = validator().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn fatal_with_mismatched_ref_id() {
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_fatal(Some("wrong-run"), "err"),
    ];
    let errors = validator().validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn event_empty_ref_id_sequence_error() {
    let env = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "x".into(),
            },
            ext: None,
        },
    };
    let result = validator().validate(&env);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Unknown Envelope Type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unknown_envelope_type_parse_error() {
    let line = r#"{"t":"unknown_type","data":"something"}"#;
    let result = decode(line);
    assert!(
        result.is_err(),
        "unknown envelope type should fail to parse"
    );
}

#[test]
fn envelope_missing_t_field() {
    let line = r#"{"type":"hello","contract_version":"abp/v0.1"}"#;
    let result = decode(line);
    assert!(
        result.is_err(),
        "envelope without 't' field should fail to parse"
    );
}

#[test]
fn envelope_wrong_tag_field_name() {
    let line = r#"{"type":"event","ref_id":"run-1","event":{}}"#;
    let result = decode(line);
    assert!(result.is_err(), "using 'type' instead of 't' should fail");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Malformed JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn malformed_json_not_json() {
    let result = decode("this is not json");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn malformed_json_truncated() {
    let result = decode(r#"{"t":"hello","contract_ver"#);
    assert!(result.is_err());
}

#[test]
fn malformed_json_missing_required_field() {
    let line = r#"{"t":"hello","backend":{"id":"test"}}"#;
    let result = decode(line);
    assert!(result.is_err(), "missing contract_version should fail");
}

#[test]
fn malformed_json_wrong_value_type() {
    let line = r#"{"t":"hello","contract_version":42,"backend":{"id":"test"},"capabilities":{}}"#;
    let result = decode(line);
    assert!(result.is_err());
}

#[test]
fn malformed_json_empty_string() {
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}

#[test]
fn malformed_json_blank_lines_skipped() {
    let input = format!("\n\n{}\n\n", encode(&make_hello()).trim());
    let results = decode_stream(&input);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn malformed_json_mixed_valid_invalid() {
    let valid = encode(&make_fatal(None, "err"));
    let input = format!("{valid}not json\n{valid}");
    let results = decode_stream(&input);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Large Payloads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_tool_output_event() {
    let large_output = "x".repeat(1_000_000);
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tool-1".into()),
            output: json!(large_output),
            is_error: false,
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult { output, .. } = &event.kind {
            assert_eq!(output.as_str().unwrap().len(), 1_000_000);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_payload_validation_warning() {
    let huge = "x".repeat(11 * 1024 * 1024);
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "big".into(),
            tool_use_id: None,
            output: json!(huge),
            is_error: false,
        },
    );
    let result = validator().validate(&env);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::LargePayload { .. }))
    );
}

#[test]
fn large_assistant_message() {
    let big_text = "word ".repeat(100_000);
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: big_text.clone(),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text.len(), big_text.len());
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Empty / Minimal Valid Events
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn minimal_run_started_event() {
    let env = make_event(
        "r",
        AgentEventKind::RunStarted {
            message: String::new(),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn minimal_assistant_delta() {
    let env = make_event(
        "r",
        AgentEventKind::AssistantDelta {
            text: String::new(),
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_tool_call_event() {
    let env = make_event(
        "r",
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_tool_result_event() {
    let env = make_event(
        "r",
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_file_changed_event() {
    let env = make_event(
        "r",
        AgentEventKind::FileChanged {
            path: "a.txt".into(),
            summary: String::new(),
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_command_executed_event() {
    let env = make_event(
        "r",
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: None,
            output_preview: None,
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_warning_event() {
    let env = make_event(
        "r",
        AgentEventKind::Warning {
            message: "warn".into(),
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

#[test]
fn minimal_error_event() {
    let env = make_event(
        "r",
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
    );
    let line = encode(&env);
    assert!(decode(&line).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Mixed Event Types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_with_all_event_types() {
    let run_id = "run-mix";
    let events = vec![
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta { text: "tok".into() },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "file.txt"}),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("tc-1".into()),
                output: json!("contents"),
                is_error: false,
            },
        ),
        make_event(
            run_id,
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::Warning {
                message: "dep outdated".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::Error {
                message: "compile err".into(),
                error_code: None,
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
    ];

    let stream = encode_stream(&events);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), events.len());
    for r in &decoded {
        assert!(r.is_ok());
    }
}

#[test]
fn full_protocol_sequence_all_event_types() {
    let run_id = "run-full";
    let mut seq = vec![make_hello(), make_run(run_id)];
    seq.push(make_event(
        run_id,
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
    ));
    seq.push(make_event(
        run_id,
        AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.txt", "content": "data"}),
        },
    ));
    seq.push(make_event(
        run_id,
        AgentEventKind::ToolResult {
            tool_name: "write".into(),
            tool_use_id: Some("t1".into()),
            output: json!("ok"),
            is_error: false,
        },
    ));
    seq.push(make_event(
        run_id,
        AgentEventKind::FileChanged {
            path: "a.txt".into(),
            summary: "created".into(),
        },
    ));
    seq.push(make_event(
        run_id,
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ));
    seq.push(make_final(run_id));

    let errors = validator().validate_sequence(&seq);
    assert!(errors.is_empty(), "expected valid sequence: {errors:?}");

    let stream = encode_stream(&seq);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), seq.len());
}

#[test]
fn mixed_delta_and_message_events() {
    let run_id = "run-txt";
    let events = vec![
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: " world".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantMessage {
                text: "Hello world".into(),
            },
        ),
    ];
    let stream = encode_stream(&events);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), 3);
    for r in decoded {
        assert!(r.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Contract Version Mismatch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert!(parse_version("invalid").is_none());
    assert!(parse_version("abp/0.1").is_none());
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("").is_none());
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v2.0"));
}

#[test]
fn incompatible_version_invalid_format() {
    assert!(!is_compatible_version("not-valid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

#[test]
fn hello_with_future_compatible_version() {
    let env = make_hello_with_version("abp/v0.2");
    let result = validator().validate(&env);
    assert!(
        result.valid,
        "future minor version should still be parseable"
    );
}

#[test]
fn hello_with_incompatible_major_version() {
    let env = make_hello_with_version("abp/v1.0");
    let result = validator().validate(&env);
    // The envelope itself is valid, but the version may be incompatible
    assert!(result.valid, "the envelope structure itself is valid");
    // Check compatibility
    assert!(!is_compatible_version("abp/v1.0", CONTRACT_VERSION));
}

#[test]
fn version_mismatch_detection_in_hello() {
    let env = make_hello_with_version("abp/v0.1");
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert!(is_compatible_version(contract_version, CONTRACT_VERSION));
    }

    let env2 = make_hello_with_version("abp/v1.0");
    if let Envelope::Hello {
        contract_version, ..
    } = &env2
    {
        assert!(!is_compatible_version(contract_version, CONTRACT_VERSION));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: JSONL codec stream tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decode_stream_preserves_order() {
    let envs = vec![
        make_hello(),
        make_run("run-1"),
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "hi".into(),
            },
        ),
        make_final("run-1"),
    ];
    let stream = encode_stream(&envs);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), 4);

    assert!(matches!(
        decoded[0].as_ref().unwrap(),
        Envelope::Hello { .. }
    ));
    assert!(matches!(decoded[1].as_ref().unwrap(), Envelope::Run { .. }));
    assert!(matches!(
        decoded[2].as_ref().unwrap(),
        Envelope::Event { .. }
    ));
    assert!(matches!(
        decoded[3].as_ref().unwrap(),
        Envelope::Final { .. }
    ));
}

#[test]
fn encode_to_writer_works() {
    let mut buf = Vec::new();
    let env = make_hello();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains(r#""t":"hello""#));
    assert!(s.ends_with('\n'));
}

#[test]
fn encode_many_to_writer_works() {
    let mut buf = Vec::new();
    let envs = [make_hello(), make_fatal(None, "err")];
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Event extension field (ext)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_with_ext_field_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));

    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(event.ext.is_some());
        let ext = event.ext.unwrap();
        assert!(ext.contains_key("raw_message"));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_without_ext_field_omits_in_json() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage { text: "hi".into() },
    );
    let line = encode(&env);
    // ext is None, so skip_serializing_if should omit it
    assert!(!line.contains("\"ext\""));
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Serde tag invariants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_envelope_variants_use_tag_t() {
    let envs: Vec<Envelope> = vec![
        make_hello(),
        make_run("r"),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
        ),
        make_final("r"),
        make_fatal(Some("r"), "e"),
    ];
    for env in &envs {
        let line = encode(env);
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert!(
            parsed.get("t").is_some(),
            "envelope must have 't' tag: {line}"
        );
        assert!(
            parsed.get("type").is_none()
                || parsed
                    .get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| {
                        // AgentEventKind uses "type" for its own tag inside event.kind
                        // That's expected and correct (it's flattened inside the event)
                        !["hello", "run", "event", "final", "fatal"].contains(&s)
                    }),
            "envelope-level tag must not be 'type': {line}"
        );
    }
}

#[test]
fn agent_event_kind_uses_tag_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        json.contains(r#""type":"run_started""#),
        "AgentEventKind should use 'type' tag: {json}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: ProtocolError variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("}{bad").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.to_string().contains("invalid JSON"));
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("test violation".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Receipt integrity in final envelopes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn final_receipt_hash_roundtrip() {
    let env = make_final("run-1");
    if let Envelope::Final { receipt, .. } = env {
        let hashed = receipt.with_hash().unwrap();
        assert!(hashed.receipt_sha256.is_some());
        assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_outcome_variants() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let now = Utc::now();
        let env = Envelope::Final {
            ref_id: "run-1".into(),
            receipt: Receipt {
                meta: RunMetadata {
                    run_id: Uuid::nil(),
                    work_order_id: Uuid::nil(),
                    contract_version: CONTRACT_VERSION.into(),
                    started_at: now,
                    finished_at: now,
                    duration_ms: 0,
                },
                backend: test_backend(),
                capabilities: test_capabilities(),
                mode: ExecutionMode::default(),
                usage_raw: json!({}),
                usage: UsageNormalized::default(),
                trace: vec![],
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome: outcome.clone(),
                receipt_sha256: None,
            },
        };
        let line = encode(&env);
        let decoded = decode(&line).unwrap();
        if let Envelope::Final { receipt, .. } = decoded {
            assert_eq!(receipt.outcome, outcome);
        } else {
            panic!("expected Final");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: WorkOrder fields in run envelope
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_work_order_preserves_all_fields() {
    let mut wo = test_work_order();
    wo.task = "complex task".into();
    wo.lane = ExecutionLane::WorkspaceFirst;
    wo.config.model = Some("gpt-4".into());
    wo.config.max_turns = Some(5);
    wo.config.max_budget_usd = Some(1.50);

    let env = Envelope::Run {
        id: "run-fields".into(),
        work_order: wo,
    };
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-fields");
        assert_eq!(work_order.task, "complex task");
        assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(work_order.config.max_turns, Some(5));
        assert!((work_order.config.max_budget_usd.unwrap() - 1.50).abs() < f64::EPSILON);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_work_order_with_policy() {
    let mut wo = test_work_order();
    wo.policy.allowed_tools = vec!["read".into(), "write".into()];
    wo.policy.deny_write = vec!["*.secret".into()];

    let env = Envelope::Run {
        id: "run-policy".into(),
        work_order: wo,
    };
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.policy.allowed_tools, vec!["read", "write"]);
        assert_eq!(work_order.policy.deny_write, vec!["*.secret"]);
    } else {
        panic!("expected Run");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Tool call/result correlation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_and_result_ids_correlate() {
    let tool_use_id = "tu-abc";
    let call = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some(tool_use_id.into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        },
    );
    let result = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some(tool_use_id.into()),
            output: json!("file1.txt\nfile2.txt"),
            is_error: false,
        },
    );

    let stream = encode_stream(&[call, result]);
    let decoded = decode_stream(&stream);
    assert_eq!(decoded.len(), 2);

    let call_decoded = decoded[0].as_ref().unwrap();
    let result_decoded = decoded[1].as_ref().unwrap();

    let call_id = if let Envelope::Event { event, .. } = call_decoded {
        if let AgentEventKind::ToolCall { tool_use_id, .. } = &event.kind {
            tool_use_id.clone()
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    };

    let result_id = if let Envelope::Event { event, .. } = result_decoded {
        if let AgentEventKind::ToolResult { tool_use_id, .. } = &event.kind {
            tool_use_id.clone()
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    };

    assert_eq!(call_id, result_id);
}

#[test]
fn tool_result_error_flag() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t-1".into()),
            output: json!("permission denied"),
            is_error: true,
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult { is_error, .. } = &event.kind {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Contract version constant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_parseable() {
    let (major, minor) = parse_version(CONTRACT_VERSION).expect("CONTRACT_VERSION must parse");
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: RawFrame re-export
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn raw_frame_type_accessible() {
    // Verify the re-export compiles and is usable
    let _frame: Option<RawFrame> = None;
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Edge cases in serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_in_event_text() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "Hello 🌍 café résumé 日本語".into(),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert!(text.contains("🌍"));
            assert!(text.contains("café"));
            assert!(text.contains("日本語"));
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn special_characters_in_error_message() {
    let msg = r#"error: "unexpected token" at line 5\ncaused by: <nil>"#;
    let env = make_fatal(Some("run-1"), msg);
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, msg);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn json_with_nested_objects_in_tool_input() {
    let input = json!({
        "path": "src/main.rs",
        "content": "fn main() {}",
        "metadata": {
            "language": "rust",
            "lines": [1, 2, 3],
            "nested": {"deep": true}
        }
    });
    let env = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tc-nested".into()),
            parent_tool_use_id: None,
            input: input.clone(),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            input: decoded_input,
            ..
        } = &event.kind
        {
            assert_eq!(decoded_input, &input);
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn parent_tool_use_id_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "sub_tool".into(),
            tool_use_id: Some("child-1".into()),
            parent_tool_use_id: Some("parent-1".into()),
            input: json!({}),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            parent_tool_use_id, ..
        } = &event.kind
        {
            assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1"));
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn error_event_with_error_code() {
    let env = make_event(
        "run-1",
        AgentEventKind::Error {
            message: "rate limited".into(),
            error_code: Some(abp_error::ErrorCode::ProtocolInvalidEnvelope),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Error { error_code, .. } = &event.kind {
            assert!(error_code.is_some());
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn command_executed_with_all_fields() {
    let env = make_event(
        "run-1",
        AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(1),
            output_preview: Some("error[E0308]: mismatched types".into()),
        },
    );
    let line = encode(&env);
    let decoded = decode(&line).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = &event.kind
        {
            assert_eq!(command, "cargo build");
            assert_eq!(*exit_code, Some(1));
            assert!(
                output_preview
                    .as_ref()
                    .unwrap()
                    .contains("mismatched types")
            );
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}
