#![allow(dead_code, unused_imports, unused_variables)]

//! Conformance test harness for validating sidecar protocol implementations.
//!
//! Covers:
//! - Protocol sequence tests (hello → run → event* → final/fatal)
//! - Envelope format tests (JSON structure with "t" discriminator)
//! - Contract version tests (matching, mismatched, missing)
//! - Capability negotiation tests
//! - Error handling tests (malformed JSON, unknown types, oversized)
//! - Receipt validation tests (hash integrity, determinism, tamper detection)

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    ReceiptBuilder, RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, receipt_hash,
};
use abp_protocol::{
    Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version,
    validate::{EnvelopeValidator, SequenceError, ValidationError, ValidationWarning},
};
use abp_sidecar_proto::negotiation::{
    CapabilityAdvertisement, HandshakeValidator, NegotiationResult, ProtocolVersion,
    negotiate_version,
};
use abp_sidecar_proto::state_machine::{ProtocolState, ProtocolStateMachine, TransitionError};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "conformance-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn hello_env() -> Envelope {
    Envelope::hello(test_identity(), test_capabilities())
}

fn hello_env_with_version(version: &str) -> Envelope {
    Envelope::Hello {
        contract_version: version.to_string(),
        backend: test_identity(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    }
}

fn run_env() -> Envelope {
    let wo = WorkOrderBuilder::new("conformance task").build();
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    }
}

fn run_env_with_id(id: &str) -> Envelope {
    let wo = WorkOrderBuilder::new("conformance task").build();
    Envelope::Run {
        id: id.to_string(),
        work_order: wo,
    }
}

fn event_env(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.to_string(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello from sidecar".into(),
            },
            ext: None,
        },
    }
}

fn delta_event_env(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.to_string(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.into() },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("conformance-sidecar")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.to_string(),
        receipt,
    }
}

fn fatal_env(ref_id: Option<&str>) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(|s| s.to_string()),
        error: "test error".into(),
        error_code: None,
    }
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Protocol sequence tests
// ═══════════════════════════════════════════════════════════════════════════

mod protocol_sequence {
    use super::*;

    #[test]
    fn valid_hello_run_events_final() {
        let mut sm = ProtocolStateMachine::new();
        assert_eq!(sm.state(), ProtocolState::AwaitingHello);

        sm.advance(&hello_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::AwaitingRun);

        sm.advance(&run_env()).unwrap();
        assert_eq!(sm.state(), ProtocolState::Streaming);

        sm.advance(&event_env("r1")).unwrap();
        sm.advance(&event_env("r1")).unwrap();
        sm.advance(&event_env("r1")).unwrap();
        assert_eq!(sm.events_seen(), 3);

        sm.advance(&final_env("r1")).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
        assert!(sm.state().is_terminal());
    }

    #[test]
    fn valid_hello_run_no_events_final() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&final_env("r")).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
        assert_eq!(sm.events_seen(), 0);
    }

    #[test]
    fn valid_hello_run_fatal_early_abort() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&fatal_env(Some("r"))).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn valid_fatal_at_any_point_awaiting_hello() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&fatal_env(None)).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn valid_fatal_at_any_point_awaiting_run() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&fatal_env(None)).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn valid_fatal_during_streaming() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&event_env("r")).unwrap();
        sm.advance(&fatal_env(Some("r"))).unwrap();
        assert_eq!(sm.state(), ProtocolState::Terminated);
    }

    #[test]
    fn invalid_missing_hello_event_first() {
        let mut sm = ProtocolStateMachine::new();
        let err = sm.advance(&event_env("r")).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("event"));
    }

    #[test]
    fn invalid_missing_hello_run_first() {
        let mut sm = ProtocolStateMachine::new();
        let err = sm.advance(&run_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("run"));
    }

    #[test]
    fn invalid_event_before_run() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        let err = sm.advance(&event_env("r")).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("event"));
    }

    #[test]
    fn invalid_multiple_hello_messages() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        let err = sm.advance(&hello_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
        assert!(err.to_string().contains("hello"));
    }

    #[test]
    fn invalid_run_after_final() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&final_env("r")).unwrap();
        let _err = sm.advance(&run_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
    }

    #[test]
    fn invalid_event_after_final() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&final_env("r")).unwrap();
        let _err = sm.advance(&event_env("r")).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
    }

    #[test]
    fn invalid_hello_after_run() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        let _err = sm.advance(&hello_env()).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
    }

    #[test]
    fn invalid_final_before_run() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        let _err = sm.advance(&final_env("r")).unwrap_err();
        assert_eq!(sm.state(), ProtocolState::Error);
    }

    #[test]
    fn state_machine_reset_returns_to_initial() {
        let mut sm = ProtocolStateMachine::new();
        sm.advance(&hello_env()).unwrap();
        sm.advance(&run_env()).unwrap();
        sm.advance(&event_env("r")).unwrap();
        sm.reset();
        assert_eq!(sm.state(), ProtocolState::AwaitingHello);
        assert_eq!(sm.events_seen(), 0);
    }

    #[test]
    fn sequence_validator_valid_full_sequence() {
        let run_id = "run-123";
        let seq = vec![
            hello_env(),
            run_env_with_id(run_id),
            event_env(run_id),
            event_env(run_id),
            final_env(run_id),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn sequence_validator_missing_hello() {
        let seq = vec![run_env_with_id("r"), final_env("r")];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::MissingHello));
    }

    #[test]
    fn sequence_validator_missing_terminal() {
        let seq = vec![hello_env(), run_env_with_id("r"), event_env("r")];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn sequence_validator_empty_sequence() {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        assert!(errors.contains(&SequenceError::MissingHello));
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn sequence_validator_out_of_order_event() {
        let seq = vec![
            hello_env(),
            event_env("r"),
            run_env_with_id("r"),
            final_env("r"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.contains(&SequenceError::OutOfOrderEvents));
    }

    #[test]
    fn sequence_validator_ref_id_mismatch() {
        let seq = vec![
            hello_env(),
            run_env_with_id("run-1"),
            event_env("wrong-id"),
            final_env("run-1"),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Envelope format tests
// ═══════════════════════════════════════════════════════════════════════════

mod envelope_format {
    use super::*;

    #[test]
    fn hello_envelope_has_t_discriminator() {
        let json = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(json.contains(r#""t":"hello""#));
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn hello_contains_contract_version() {
        let json = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn hello_contains_backend_identity() {
        let json = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(json.contains("conformance-sidecar"));
    }

    #[test]
    fn hello_contains_capabilities() {
        let json = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(json.contains("streaming"));
    }

    #[test]
    fn hello_roundtrip_serde() {
        let envelope = hello_env();
        let json = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello {
                contract_version,
                backend,
                capabilities,
                mode,
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert_eq!(backend.id, "conformance-sidecar");
                assert!(capabilities.contains_key(&Capability::Streaming));
                assert_eq!(mode, ExecutionMode::Mapped);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn run_envelope_has_t_discriminator() {
        let json = JsonlCodec::encode(&run_env()).unwrap();
        assert!(json.contains(r#""t":"run""#));
    }

    #[test]
    fn run_envelope_contains_work_order() {
        let wo = WorkOrderBuilder::new("test task").build();
        let envelope = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo.clone(),
        };
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.contains("test task"));
        assert!(json.contains(&wo.id.to_string()));
    }

    #[test]
    fn run_envelope_roundtrip() {
        let wo = WorkOrderBuilder::new("roundtrip task").build();
        let id = wo.id.to_string();
        let envelope = Envelope::Run {
            id: id.clone(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run {
                id: decoded_id,
                work_order,
            } => {
                assert_eq!(decoded_id, id);
                assert_eq!(work_order.task, "roundtrip task");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn event_envelope_all_agent_event_variants() {
        let variants: Vec<AgentEventKind> = vec![
            AgentEventKind::RunStarted {
                message: "started".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta {
                text: "token".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "foo.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            AgentEventKind::Warning {
                message: "heads up".into(),
            },
            AgentEventKind::Error {
                message: "bad thing".into(),
                error_code: None,
            },
        ];

        for kind in variants {
            let event = make_agent_event(kind);
            let envelope = Envelope::Event {
                ref_id: "run-all-variants".into(),
                event,
            };
            let json = JsonlCodec::encode(&envelope).unwrap();
            assert!(json.contains(r#""t":"event""#));
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            assert!(matches!(decoded, Envelope::Event { .. }));
        }
    }

    #[test]
    fn final_envelope_with_receipt() {
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build();
        let envelope = Envelope::Final {
            ref_id: "r".into(),
            receipt,
        };
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.contains(r#""t":"final""#));
        assert!(json.contains(r#""ref_id":"r""#));

        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r");
                assert_eq!(receipt.outcome, Outcome::Complete);
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn fatal_envelope_with_error() {
        let envelope = Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "out of memory".into(),
            error_code: None,
        };
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.contains(r#""t":"fatal""#));
        assert!(json.contains("out of memory"));

        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal {
                ref_id,
                error,
                error_code,
            } => {
                assert_eq!(ref_id, Some("r".into()));
                assert_eq!(error, "out of memory");
                assert!(error_code.is_none());
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn fatal_envelope_without_ref_id() {
        let envelope = Envelope::Fatal {
            ref_id: None,
            error: "init failure".into(),
            error_code: None,
        };
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.contains(r#""ref_id":null"#));
    }

    #[test]
    fn fatal_envelope_with_error_code() {
        let envelope = Envelope::fatal_with_code(
            Some("r".into()),
            "protocol violation",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.contains("error_code"));

        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert!(decoded.error_code().is_some());
    }

    #[test]
    fn encode_produces_newline_terminated_json() {
        let envelopes: Vec<Envelope> = vec![hello_env(), run_env(), event_env("r"), final_env("r")];
        for env in &envelopes {
            let json = JsonlCodec::encode(env).unwrap();
            assert!(json.ends_with('\n'), "envelope must be newline-terminated");
            let _: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        }
    }

    #[test]
    fn decode_stream_parses_multiple_envelopes() {
        let mut buf = String::new();
        buf.push_str(&JsonlCodec::encode(&hello_env()).unwrap());
        buf.push_str(&JsonlCodec::encode(&fatal_env(None)).unwrap());

        let reader = std::io::BufReader::new(buf.as_bytes());
        let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
        assert!(matches!(envelopes[0], Envelope::Hello { .. }));
        assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = format!(
            "\n{}\n\n{}\n",
            JsonlCodec::encode(&hello_env()).unwrap().trim(),
            JsonlCodec::encode(&fatal_env(None)).unwrap().trim(),
        );
        let reader = std::io::BufReader::new(input.as_bytes());
        let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn envelope_validator_hello_valid() {
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&hello_env());
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn envelope_validator_hello_empty_version() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: "".into(),
            backend: test_identity(),
            capabilities: test_capabilities(),
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
    fn envelope_validator_hello_invalid_version_format() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Hello {
            contract_version: "not-a-version".into(),
            backend: test_identity(),
            capabilities: test_capabilities(),
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
    fn envelope_validator_hello_empty_backend_id() {
        let validator = EnvelopeValidator::new();
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
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| matches!(
            e,
            ValidationError::EmptyField { field } if field == "backend.id"
        )));
    }

    #[test]
    fn envelope_validator_run_empty_id() {
        let validator = EnvelopeValidator::new();
        let wo = WorkOrderBuilder::new("task").build();
        let env = Envelope::Run {
            id: "".into(),
            work_order: wo,
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn envelope_validator_fatal_empty_error() {
        let validator = EnvelopeValidator::new();
        let env = Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "".into(),
            error_code: None,
        };
        let result = validator.validate(&env);
        assert!(!result.valid);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Contract version tests
// ═══════════════════════════════════════════════════════════════════════════

mod contract_version {
    use super::*;

    #[test]
    fn parse_current_version() {
        let (major, minor) = parse_version(CONTRACT_VERSION).unwrap();
        assert_eq!(major, 0);
        assert_eq!(minor, 1);
    }

    #[test]
    fn matching_versions_accept() {
        assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
    }

    #[test]
    fn same_major_different_minor_accept() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.99", "abp/v0.1"));
    }

    #[test]
    fn mismatched_major_version_reject() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
        assert!(!is_compatible_version("abp/v2.0", "abp/v0.1"));
    }

    #[test]
    fn missing_version_reject() {
        assert!(!is_compatible_version("", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", ""));
        assert!(!is_compatible_version("invalid", "abp/v0.1"));
    }

    #[test]
    fn parse_version_invalid_formats() {
        assert!(parse_version("").is_none());
        assert!(parse_version("invalid").is_none());
        assert!(parse_version("abp/v").is_none());
        assert!(parse_version("abp/vX.Y").is_none());
        assert!(parse_version("v0.1").is_none());
    }

    #[test]
    fn protocol_version_struct_current() {
        let v = ProtocolVersion::current();
        assert_eq!(v.to_version_string(), CONTRACT_VERSION);
    }

    #[test]
    fn protocol_version_struct_compatibility() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        let v10 = ProtocolVersion { major: 1, minor: 0 };
        assert!(v01.is_compatible_with(&v02));
        assert!(!v01.is_compatible_with(&v10));
    }

    #[test]
    fn negotiate_same_version_agrees() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&v, &v);
        assert!(result.is_agreed());
        assert_eq!(result, NegotiationResult::Agreed(v));
    }

    #[test]
    fn negotiate_different_minor_agrees_to_lower() {
        let ours = ProtocolVersion { major: 0, minor: 3 };
        let theirs = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&ours, &theirs);
        assert_eq!(
            result,
            NegotiationResult::Agreed(ProtocolVersion { major: 0, minor: 1 })
        );
    }

    #[test]
    fn negotiate_incompatible_major_rejects() {
        let ours = ProtocolVersion { major: 0, minor: 1 };
        let theirs = ProtocolVersion { major: 1, minor: 0 };
        let result = negotiate_version(&ours, &theirs);
        assert!(!result.is_agreed());
        assert!(matches!(result, NegotiationResult::Incompatible { .. }));
    }

    #[test]
    fn future_version_same_major_compatible() {
        assert!(is_compatible_version("abp/v0.99", CONTRACT_VERSION));
    }

    #[test]
    fn handshake_validator_rejects_incompatible_version() {
        let hello = hello_env_with_version("abp/v99.0");
        let validator = HandshakeValidator::new();
        let err = validator.validate_hello(&hello).unwrap_err();
        assert!(err.to_string().contains("incompatible"));
    }

    #[test]
    fn handshake_validator_accepts_compatible_version() {
        let hello = hello_env_with_version(CONTRACT_VERSION);
        let validator = HandshakeValidator::new();
        let ad = validator.validate_hello(&hello).unwrap();
        assert_eq!(ad.protocol_version, CONTRACT_VERSION);
    }

    #[test]
    fn handshake_validator_rejects_non_hello_envelope() {
        let event = event_env("r");
        let validator = HandshakeValidator::new();
        assert!(validator.validate_hello(&event).is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability negotiation tests
// ═══════════════════════════════════════════════════════════════════════════

mod capability_negotiation {
    use super::*;

    #[test]
    fn sidecar_declares_supported_capabilities() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Emulated);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);
        assert!(ad.has_capability(&Capability::Streaming));
        assert!(ad.has_capability(&Capability::ToolRead));
        assert!(!ad.has_capability(&Capability::ToolWrite));
    }

    #[test]
    fn control_plane_validates_against_requirements_pass() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn missing_required_capability_reject_run() {
        let ad = CapabilityAdvertisement::new(test_identity(), CapabilityManifest::new());
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Any,
            }],
        };
        assert!(!ad.satisfies(&reqs));
    }

    #[test]
    fn extra_capabilities_ignored_forward_compatible() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        caps.insert(Capability::ToolWrite, SupportLevel::Native);
        caps.insert(Capability::ToolBash, SupportLevel::Native);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn emulated_satisfies_emulated_requirement() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Emulated);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn emulated_does_not_satisfy_native_requirement() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Emulated);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        assert!(!ad.satisfies(&reqs));
    }

    #[test]
    fn handshake_validator_checks_required_capabilities() {
        let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
        let validator = HandshakeValidator::new().require_capabilities(vec![Capability::Streaming]);
        let err = validator.validate_hello(&hello).unwrap_err();
        assert!(err.to_string().contains("missing required capability"));
    }

    #[test]
    fn handshake_validator_passes_when_capabilities_present() {
        let hello = Envelope::hello(test_identity(), test_capabilities());
        let validator = HandshakeValidator::new().require_capabilities(vec![Capability::Streaming]);
        assert!(validator.validate_hello(&hello).is_ok());
    }

    #[test]
    fn empty_requirements_always_satisfied() {
        let ad = CapabilityAdvertisement::new(test_identity(), CapabilityManifest::new());
        let reqs = CapabilityRequirements::default();
        assert!(ad.satisfies(&reqs));
    }

    #[test]
    fn multiple_required_capabilities_all_must_match() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let ad = CapabilityAdvertisement::new(test_identity(), caps);

        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Any,
                },
            ],
        };
        assert!(!ad.satisfies(&reqs));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Error handling tests
// ═══════════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[test]
    fn malformed_json_returns_error() {
        let result = JsonlCodec::decode("not valid json at all");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn incomplete_envelope_missing_fields() {
        let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn incomplete_envelope_missing_t_field() {
        let result = JsonlCodec::decode(r#"{"error":"boom"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_envelope_type_errors() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":"foo"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn empty_string_decode_errors() {
        let result = JsonlCodec::decode("");
        assert!(result.is_err());
    }

    #[test]
    fn partial_json_decode_errors() {
        let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1""#);
        assert!(result.is_err());
    }

    #[test]
    fn protocol_error_display_json() {
        let err = JsonlCodec::decode("bad json").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("JSON") || msg.contains("json"));
    }

    #[test]
    fn protocol_error_violation() {
        let err = ProtocolError::Violation("test violation".into());
        assert!(err.to_string().contains("test violation"));
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
        assert!(err.to_string().contains("hello"));
        assert!(err.to_string().contains("run"));
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );
    }

    #[test]
    fn hello_envelope_with_extra_json_fields_accepted() {
        // Forward compatibility: extra fields should be ignored by serde
        let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored"}"#;
        let result = JsonlCodec::decode(json);
        assert!(result.is_ok());
    }

    #[test]
    fn valid_json_but_wrong_structure() {
        let result = JsonlCodec::decode(r#"[1, 2, 3]"#);
        assert!(result.is_err());
    }

    #[test]
    fn json_with_null_required_field() {
        let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":null}"#);
        assert!(result.is_err());
    }

    #[test]
    fn transition_error_display() {
        let err = TransitionError {
            state: ProtocolState::AwaitingHello,
            envelope_type: "event".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("event"));
        assert!(msg.contains("awaiting_hello") || msg.contains("error"));
    }

    #[test]
    fn validation_error_display_formats() {
        let e1 = ValidationError::MissingField {
            field: "contract_version".into(),
        };
        assert!(e1.to_string().contains("contract_version"));

        let e2 = ValidationError::InvalidVersion {
            version: "bad".into(),
        };
        assert!(e2.to_string().contains("bad"));

        let e3 = ValidationError::EmptyField {
            field: "backend.id".into(),
        };
        assert!(e3.to_string().contains("backend.id"));
    }

    #[test]
    fn sequence_error_display_formats() {
        assert!(SequenceError::MissingHello.to_string().contains("Hello"));
        assert!(
            SequenceError::MissingTerminal
                .to_string()
                .contains("terminal")
        );
        assert!(
            SequenceError::MultipleTerminals
                .to_string()
                .contains("multiple")
        );
        assert!(
            SequenceError::OutOfOrderEvents
                .to_string()
                .contains("Event")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Receipt validation tests
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_validation {
    use super::*;

    #[test]
    fn receipt_hash_matches_content() {
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();

        assert!(receipt.receipt_sha256.is_some());
        let stored_hash = receipt.receipt_sha256.as_ref().unwrap();

        let recomputed = receipt_hash(&receipt).unwrap();
        assert_eq!(stored_hash, &recomputed);
    }

    #[test]
    fn receipt_hash_is_deterministic() {
        let receipt = ReceiptBuilder::new("deterministic-test")
            .outcome(Outcome::Complete)
            .build();

        let hash1 = receipt_hash(&receipt).unwrap();
        let hash2 = receipt_hash(&receipt).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex digest
    }

    #[test]
    fn tampered_receipt_detected() {
        let receipt = ReceiptBuilder::new("tamper-test")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();

        let original_hash = receipt.receipt_sha256.clone().unwrap();

        let mut tampered = receipt;
        tampered.outcome = Outcome::Failed;

        let tampered_hash = receipt_hash(&tampered).unwrap();
        assert_ne!(
            original_hash, tampered_hash,
            "tampered receipt must produce different hash"
        );
    }

    #[test]
    fn receipt_with_null_hash_field_for_hashing() {
        let mut receipt = ReceiptBuilder::new("null-hash-test")
            .outcome(Outcome::Complete)
            .build();

        // Hash with None
        let hash_none = receipt_hash(&receipt).unwrap();

        // Hash with Some — should produce same result since it's nulled before hashing
        receipt.receipt_sha256 = Some("bogus-hash-value".into());
        let hash_some = receipt_hash(&receipt).unwrap();

        assert_eq!(
            hash_none, hash_some,
            "hash must be independent of receipt_sha256 field"
        );
    }

    #[test]
    fn receipt_hash_different_backends_produce_different_hashes() {
        let r1 = ReceiptBuilder::new("backend-a")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("backend-b")
            .outcome(Outcome::Complete)
            .build();

        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_hash_different_outcomes_produce_different_hashes() {
        let r1 = ReceiptBuilder::new("same-backend")
            .outcome(Outcome::Complete)
            .build();
        let mut r2 = ReceiptBuilder::new("same-backend")
            .outcome(Outcome::Failed)
            .build();
        // Align run_ids and timestamps for a fair comparison
        r2.meta.run_id = r1.meta.run_id;
        r2.meta.started_at = r1.meta.started_at;
        r2.meta.finished_at = r1.meta.finished_at;

        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn receipt_with_hash_then_roundtrip() {
        let receipt = ReceiptBuilder::new("roundtrip-hash")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();

        let json_str = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();

        assert_eq!(
            receipt.receipt_sha256, deserialized.receipt_sha256,
            "hash must survive serialization roundtrip"
        );

        let recomputed = receipt_hash(&deserialized).unwrap();
        assert_eq!(deserialized.receipt_sha256.as_ref().unwrap(), &recomputed);
    }

    #[test]
    fn receipt_in_final_envelope_roundtrip() {
        let receipt = ReceiptBuilder::new("final-envelope-receipt")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();

        let hash = receipt.receipt_sha256.clone().unwrap();

        let envelope = Envelope::Final {
            ref_id: "r".into(),
            receipt,
        };

        let json = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();

        match decoded {
            Envelope::Final { receipt, .. } => {
                assert_eq!(receipt.receipt_sha256.as_ref().unwrap(), &hash);
                let recomputed = receipt_hash(&receipt).unwrap();
                assert_eq!(&recomputed, &hash);
            }
            _ => panic!("expected Final envelope"),
        }
    }

    #[test]
    fn receipt_with_trace_events_hashes_consistently() {
        let event = make_agent_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        });
        let receipt = ReceiptBuilder::new("trace-test")
            .outcome(Outcome::Complete)
            .add_trace_event(event)
            .build();

        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_contract_version_is_current() {
        let receipt = ReceiptBuilder::new("version-check").build();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_builder_defaults() {
        let receipt = ReceiptBuilder::new("defaults-test").build();
        assert_eq!(receipt.backend.id, "defaults-test");
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(receipt.receipt_sha256.is_none());
        assert!(receipt.trace.is_empty());
        assert!(receipt.artifacts.is_empty());
    }
}
