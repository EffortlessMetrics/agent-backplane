// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the JSONL protocol envelope types.
//!
//! These tests exhaustively verify invariants of envelope serialization,
//! protocol ordering, and edge-case handling using manually constructed
//! inputs (no proptest dependency).

use abp_core::*;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
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

fn test_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
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

/// Build all five envelope variants for exhaustive checks.
fn all_envelope_variants() -> Vec<Envelope> {
    vec![
        Envelope::hello(test_identity(), test_capabilities()),
        Envelope::Run {
            id: "run-1".into(),
            work_order: test_work_order(),
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: test_event(),
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: test_receipt(),
        },
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "boom".into(),
            error_code: None,
        },
    ]
}

fn discriminator_for(env: &Envelope) -> &'static str {
    match env {
        Envelope::Hello { .. } => "hello",
        Envelope::Run { .. } => "run",
        Envelope::Event { .. } => "event",
        Envelope::Final { .. } => "final",
        Envelope::Fatal { .. } => "fatal",
    }
}

// ===========================================================================
// Module: envelope_invariants
// ===========================================================================
mod envelope_invariants {
    use super::*;

    #[test]
    fn every_variant_serializes_to_valid_json() {
        for env in all_envelope_variants() {
            let json = JsonlCodec::encode(&env).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            assert!(
                parsed.is_object(),
                "envelope should serialize to a JSON object"
            );
        }
    }

    #[test]
    fn every_variant_contains_t_discriminator() {
        for env in all_envelope_variants() {
            let json = JsonlCodec::encode(&env).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            assert!(
                parsed.get("t").is_some(),
                "envelope JSON must contain \"t\" field: {json}"
            );
        }
    }

    #[test]
    fn hello_has_t_hello() {
        let env = Envelope::hello(test_identity(), test_capabilities());
        let json = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["t"], "hello");
    }

    #[test]
    fn run_has_t_run() {
        let env = Envelope::Run {
            id: "r".into(),
            work_order: test_work_order(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["t"], "run");
    }

    #[test]
    fn event_has_t_event() {
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: test_event(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["t"], "event");
    }

    #[test]
    fn final_has_t_final() {
        let env = Envelope::Final {
            ref_id: "r".into(),
            receipt: test_receipt(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["t"], "final");
    }

    #[test]
    fn fatal_has_t_fatal() {
        let env = Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "err".into(),
            error_code: None,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["t"], "fatal");
    }

    #[test]
    fn roundtrip_all_variants() {
        for env in all_envelope_variants() {
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            // Re-encode and compare JSON values for structural equality.
            let json_a: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            let json_b: serde_json::Value =
                serde_json::from_str(JsonlCodec::encode(&decoded).unwrap().trim()).unwrap();
            assert_eq!(
                json_a,
                json_b,
                "roundtrip mismatch for {}",
                discriminator_for(&env)
            );
        }
    }

    #[test]
    fn ref_id_preserved_through_serialization_event() {
        let long_id = "a".repeat(1000);
        let ids = ["run-1", "run-abc-123", "", "🚀-run", long_id.as_str()];
        for id in ids {
            let env = Envelope::Event {
                ref_id: id.to_string(),
                event: test_event(),
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            match decoded {
                Envelope::Event { ref_id, .. } => assert_eq!(ref_id, id),
                other => panic!("expected Event, got {other:?}"),
            }
        }
    }

    #[test]
    fn ref_id_preserved_through_serialization_final() {
        let env = Envelope::Final {
            ref_id: "ref-xyz-456".into(),
            receipt: test_receipt(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "ref-xyz-456"),
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn ref_id_preserved_through_serialization_fatal() {
        for ref_id in [Some("run-1".to_string()), None] {
            let env = Envelope::Fatal {
                ref_id: ref_id.clone(),
                error: "err".into(),
                error_code: None,
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            match decoded {
                Envelope::Fatal {
                    ref_id: decoded_ref,
                    ..
                } => assert_eq!(decoded_ref, ref_id),
                other => panic!("expected Fatal, got {other:?}"),
            }
        }
    }

    #[test]
    fn unknown_t_value_fails_to_parse() {
        let bad_jsons = [
            r#"{"t":"unknown","data":1}"#,
            r#"{"t":"HELLO","backend":{}}"#,
            r#"{"t":"cancel","ref_id":"x"}"#,
            r#"{"t":"","error":"x"}"#,
        ];
        for json in bad_jsons {
            let result = JsonlCodec::decode(json);
            assert!(result.is_err(), "should fail for: {json}");
        }
    }

    #[test]
    fn missing_t_field_fails_to_parse() {
        let json = r#"{"ref_id":"run-1","error":"boom"}"#;
        let result = JsonlCodec::decode(json);
        assert!(result.is_err());
    }

    #[test]
    fn encode_always_ends_with_newline() {
        for env in all_envelope_variants() {
            let encoded = JsonlCodec::encode(&env).unwrap();
            assert!(
                encoded.ends_with('\n'),
                "JSONL line must end with newline for {}",
                discriminator_for(&env)
            );
        }
    }

    #[test]
    fn encode_is_single_line() {
        for env in all_envelope_variants() {
            let encoded = JsonlCodec::encode(&env).unwrap();
            let trimmed = encoded.trim_end_matches('\n');
            assert!(
                !trimmed.contains('\n'),
                "JSONL line must be a single line for {}",
                discriminator_for(&env)
            );
        }
    }

    #[test]
    fn discriminator_values_match_expected_set() {
        let expected = ["hello", "run", "event", "final", "fatal"];
        for env in all_envelope_variants() {
            let json = JsonlCodec::encode(&env).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            let t = parsed["t"].as_str().unwrap();
            assert!(expected.contains(&t), "unexpected discriminator value: {t}");
        }
    }
}

// ===========================================================================
// Module: protocol_ordering
// ===========================================================================
mod protocol_ordering {
    use super::*;

    fn hello() -> Envelope {
        Envelope::hello(test_identity(), test_capabilities())
    }

    fn run() -> Envelope {
        Envelope::Run {
            id: "run-1".into(),
            work_order: test_work_order(),
        }
    }

    fn event() -> Envelope {
        Envelope::Event {
            ref_id: "run-1".into(),
            event: test_event(),
        }
    }

    fn final_env() -> Envelope {
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: test_receipt(),
        }
    }

    fn fatal_env() -> Envelope {
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "err".into(),
            error_code: None,
        }
    }

    #[test]
    fn valid_sequence_hello_run_event_final() {
        let seq = [hello(), run(), event(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "valid sequence should have no errors: {errors:?}"
        );
    }

    #[test]
    fn valid_sequence_hello_run_final_no_events() {
        let seq = [hello(), run(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "sequence without events is valid: {errors:?}"
        );
    }

    #[test]
    fn valid_sequence_hello_run_fatal() {
        let seq = [hello(), run(), fatal_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.is_empty(), "fatal terminal is valid: {errors:?}");
    }

    #[test]
    fn multiple_events_between_run_and_final() {
        let seq = [hello(), run(), event(), event(), event(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "multiple events should be valid: {errors:?}"
        );
    }

    #[test]
    fn empty_sequence_is_invalid() {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        assert!(errors.contains(&SequenceError::MissingHello));
        assert!(errors.contains(&SequenceError::MissingTerminal));
    }

    #[test]
    fn run_without_hello_is_invalid() {
        let seq = [run(), event(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.contains(&SequenceError::MissingHello),
            "missing hello should be detected: {errors:?}"
        );
    }

    #[test]
    fn event_before_run_is_invalid() {
        let seq = [hello(), event(), run(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.contains(&SequenceError::OutOfOrderEvents),
            "event before run should be out-of-order: {errors:?}"
        );
    }

    #[test]
    fn multiple_terminals_is_invalid() {
        let seq = [hello(), run(), final_env(), fatal_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.contains(&SequenceError::MultipleTerminals),
            "multiple terminals should be detected: {errors:?}"
        );
    }

    #[test]
    fn hello_not_first_is_detected() {
        let seq = [run(), hello(), event(), final_env()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
            "hello not at position 0 should be detected: {errors:?}"
        );
    }

    #[test]
    fn ref_id_mismatch_is_detected() {
        let seq = [
            hello(),
            run(),
            Envelope::Event {
                ref_id: "wrong-id".into(),
                event: test_event(),
            },
            final_env(),
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
            "ref_id mismatch should be detected: {errors:?}"
        );
    }

    #[test]
    fn missing_terminal_is_detected() {
        let seq = [hello(), run(), event()];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(
            errors.contains(&SequenceError::MissingTerminal),
            "missing terminal should be detected: {errors:?}"
        );
    }
}

// ===========================================================================
// Module: envelope_edge_cases
// ===========================================================================
mod envelope_edge_cases {
    use super::*;

    #[test]
    fn empty_string_in_fatal_error() {
        let env = Envelope::Fatal {
            ref_id: Some(String::new()),
            error: String::new(),
            error_code: None,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id, Some(String::new()));
                assert_eq!(error, "");
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn empty_string_in_event_ref_id() {
        let env = Envelope::Event {
            ref_id: String::new(),
            event: test_event(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, ""),
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn empty_backend_id_in_hello() {
        let identity = BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        };
        let env = Envelope::hello(identity, CapabilityManifest::new());
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { backend, .. } => assert_eq!(backend.id, ""),
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn very_long_string_in_ref_id() {
        let long_id = "x".repeat(65_536);
        let env = Envelope::Event {
            ref_id: long_id.clone(),
            event: test_event(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, long_id),
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn very_long_string_in_fatal_error() {
        let long_err = "e".repeat(65_536);
        let env = Envelope::Fatal {
            ref_id: None,
            error: long_err.clone(),
            error_code: None,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error, .. } => assert_eq!(error, long_err),
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn unicode_in_all_string_fields() {
        let unicode_strings = [
            "日本語テスト",
            "émojis: 🚀🔥💻",
            "mixed: hello 世界 مرحبا",
            "zalgo: h̷̢̧̛̝͙̗̺̰̤̙̼̙͈̫̉̿̊̔̐̈́̚e̵̙̞̣̫͇̖̐̃͊̀l̸̡̺̻̮̙̖̙̰̙̈́̊̎̈́̐̈́̀̉̕ĺ̴̻̯̠̣͎̱̲̓̀̆̓̀̅̕o̵͈̘̊̏",
            "\t\r\n\\\"/special",
        ];

        for s in unicode_strings {
            // Test in fatal error field
            let env = Envelope::Fatal {
                ref_id: Some(s.to_string()),
                error: s.to_string(),
                error_code: None,
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            match decoded {
                Envelope::Fatal { ref_id, error, .. } => {
                    assert_eq!(ref_id.as_deref(), Some(s));
                    assert_eq!(error, s);
                }
                other => panic!("expected Fatal, got {other:?}"),
            }
        }
    }

    #[test]
    fn unicode_in_event_ref_id_and_message() {
        let env = Envelope::Event {
            ref_id: "ünïcödé-rün".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "こんにちは世界".into(),
                },
                ext: None,
            },
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, "ünïcödé-rün");
                match event.kind {
                    AgentEventKind::AssistantMessage { text } => {
                        assert_eq!(text, "こんにちは世界");
                    }
                    other => panic!("expected AssistantMessage, got {other:?}"),
                }
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn null_ref_id_in_fatal() {
        let json = r#"{"t":"fatal","ref_id":null,"error":"something went wrong"}"#;
        let decoded = JsonlCodec::decode(json).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, error, .. } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "something went wrong");
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn missing_optional_error_code_in_fatal() {
        let json = r#"{"t":"fatal","ref_id":"run-1","error":"oops"}"#;
        let decoded = JsonlCodec::decode(json).unwrap();
        match decoded {
            Envelope::Fatal { error_code, .. } => {
                assert!(error_code.is_none());
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[test]
    fn missing_optional_mode_defaults_in_hello() {
        // Construct JSON without "mode" field — should default to Mapped
        let json = serde_json::json!({
            "t": "hello",
            "contract_version": CONTRACT_VERSION,
            "backend": {
                "id": "test",
                "backend_version": null,
                "adapter_version": null,
            },
            "capabilities": {},
        });
        let decoded: Envelope = serde_json::from_value(json).unwrap();
        match decoded {
            Envelope::Hello { mode, .. } => {
                assert_eq!(mode, ExecutionMode::Mapped);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn extra_unknown_fields_handling() {
        // The serde tag-based enum should reject unknown variants but unknown
        // *fields within known variants* depend on the deny_unknown_fields
        // attribute. Test the actual behavior.
        let json = r#"{"t":"fatal","ref_id":null,"error":"x","extra_field":"y"}"#;
        let result = JsonlCodec::decode(json);
        // If deny_unknown_fields is set, this fails; otherwise it succeeds.
        // We verify the actual behavior is consistent.
        match result {
            Ok(Envelope::Fatal { error, .. }) => {
                assert_eq!(error, "x");
            }
            Err(_) => {
                // deny_unknown_fields is active — extra fields are rejected
            }
            _ => panic!("unexpected variant returned"),
        }
    }

    #[test]
    fn ext_field_passthrough_in_event() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            serde_json::json!({"vendor": "custom"}),
        );
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "hi".into() },
                ext: Some(ext.clone()),
            },
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => {
                assert!(event.ext.is_some());
                let decoded_ext = event.ext.unwrap();
                assert_eq!(
                    decoded_ext["raw_message"],
                    serde_json::json!({"vendor": "custom"})
                );
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn null_ext_field_omitted_in_event() {
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        };
        let json = JsonlCodec::encode(&env).unwrap();
        // ext=None should be omitted from JSON (skip_serializing_if)
        assert!(
            !json.contains("\"ext\""),
            "ext=None should be omitted from JSON"
        );
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}\n\n\n";
        let reader = std::io::BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 1);
        assert!(matches!(envelopes[0], Envelope::Fatal { .. }));
    }

    #[test]
    fn various_agent_event_kinds_in_envelope() {
        let event_kinds = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta { text: "tok".into() },
            AgentEventKind::AssistantMessage { text: "msg".into() },
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("contents"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: Some("file.txt".into()),
            },
            AgentEventKind::Warning {
                message: "careful".into(),
            },
            AgentEventKind::Error {
                message: "bad".into(),
                error_code: None,
            },
        ];

        for kind in event_kinds {
            let env = Envelope::Event {
                ref_id: "run-1".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind,
                    ext: None,
                },
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            // Re-encode to verify structural equality
            let a: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
            let b: serde_json::Value =
                serde_json::from_str(JsonlCodec::encode(&decoded).unwrap().trim()).unwrap();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn invalid_json_returns_error() {
        let bad_inputs = [
            "",
            "null",
            "42",
            "true",
            "[]",
            "{",
            "not json at all",
            r#"{"t": 42}"#,
        ];
        for input in bad_inputs {
            let result = JsonlCodec::decode(input);
            assert!(result.is_err(), "should fail for: {input:?}");
        }
    }

    #[test]
    fn execution_mode_passthrough_roundtrips_in_hello() {
        let env = Envelope::hello_with_mode(
            test_identity(),
            test_capabilities(),
            ExecutionMode::Passthrough,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            other => panic!("expected Hello, got {other:?}"),
        }
    }
}
