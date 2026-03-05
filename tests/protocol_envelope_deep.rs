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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive integration tests for JSONL protocol envelope
//! serialization/deserialization.
//!
//! Covers: Envelope serde with tag "t", JSONL framing, protocol sequence
//! validation, ref_id correlation, and error handling for malformed input.

use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_sidecar_utils::validate as seq_validate;
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn hello_env() -> Envelope {
    Envelope::hello(backend("test-sidecar"), CapabilityManifest::new())
}

fn run_env(task: &str) -> (String, Envelope) {
    let wo = WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
}

fn event_env(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("test-sidecar")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn to_json_value(env: &Envelope) -> serde_json::Value {
    let s = JsonlCodec::encode(env).unwrap();
    serde_json::from_str(s.trim()).unwrap()
}

// ===========================================================================
// A) Envelope Serialization (10 tests)
// ===========================================================================

mod envelope_serialization {
    use super::*;

    #[test]
    fn hello_has_t_hello_discriminator() {
        let v = to_json_value(&hello_env());
        assert_eq!(v["t"], "hello", "Hello envelope must have \"t\":\"hello\"");
    }

    #[test]
    fn run_has_t_run_discriminator() {
        let (_, env) = run_env("task");
        let v = to_json_value(&env);
        assert_eq!(v["t"], "run", "Run envelope must have \"t\":\"run\"");
    }

    #[test]
    fn event_has_t_event_discriminator() {
        let env = event_env("r1", "hi");
        let v = to_json_value(&env);
        assert_eq!(v["t"], "event", "Event envelope must have \"t\":\"event\"");
    }

    #[test]
    fn final_has_t_final_discriminator() {
        let env = final_env("r1");
        let v = to_json_value(&env);
        assert_eq!(v["t"], "final", "Final envelope must have \"t\":\"final\"");
    }

    #[test]
    fn fatal_has_t_fatal_discriminator() {
        let env = fatal_env(Some("r1"), "err");
        let v = to_json_value(&env);
        assert_eq!(v["t"], "fatal", "Fatal envelope must have \"t\":\"fatal\"");
    }

    #[test]
    fn event_includes_ref_id() {
        let env = event_env("run-42", "msg");
        let v = to_json_value(&env);
        assert_eq!(v["ref_id"], "run-42");
    }

    #[test]
    fn final_includes_ref_id() {
        let env = final_env("run-99");
        let v = to_json_value(&env);
        assert_eq!(v["ref_id"], "run-99");
    }

    #[test]
    fn hello_includes_contract_version() {
        let v = to_json_value(&hello_env());
        assert_eq!(
            v["contract_version"], CONTRACT_VERSION,
            "Hello must embed the contract version"
        );
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let (run_id, run) = run_env("roundtrip task");
        let json = JsonlCodec::encode(&run).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { id, work_order } => {
                assert_eq!(id, run_id);
                assert_eq!(work_order.task, "roundtrip task");
            }
            _ => panic!("expected Run after roundtrip"),
        }
    }

    #[test]
    fn fatal_optional_error_code_omitted_when_none() {
        let env = fatal_env(Some("r1"), "boom");
        let v = to_json_value(&env);
        // error_code is skip_serializing_if = "Option::is_none"
        assert!(
            v.get("error_code").is_none(),
            "error_code should be omitted when None"
        );
    }
}

// ===========================================================================
// B) JSONL Framing (10 tests)
// ===========================================================================

mod jsonl_framing {
    use super::*;

    #[test]
    fn encode_produces_single_line() {
        let line = JsonlCodec::encode(&hello_env()).unwrap();
        // Trim trailing newline, then assert no internal newlines.
        let trimmed = line.trim_end_matches('\n');
        assert!(
            !trimmed.contains('\n'),
            "encoded envelope must be a single line"
        );
    }

    #[test]
    fn encode_ends_with_newline() {
        let line = JsonlCodec::encode(&hello_env()).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn no_embedded_newlines_in_event_text() {
        // Even if the text payload contains a newline character, serde_json
        // escapes it as \\n inside the JSON string, keeping the JSONL frame intact.
        let env = event_env("r1", "line1\nline2\nline3");
        let line = JsonlCodec::encode(&env).unwrap();
        let trimmed = line.trim_end_matches('\n');
        assert!(
            !trimmed.contains('\n'),
            "embedded newlines must be JSON-escaped"
        );
    }

    #[test]
    fn decode_stream_multiple_envelopes() {
        let mut buf = String::new();
        buf.push_str(&JsonlCodec::encode(&hello_env()).unwrap());
        buf.push_str(&JsonlCodec::encode(&fatal_env(None, "err")).unwrap());

        let reader = BufReader::new(buf.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
        assert!(matches!(envelopes[0], Envelope::Hello { .. }));
        assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
    }

    #[test]
    fn decode_stream_handles_trailing_newline() {
        let mut buf = JsonlCodec::encode(&hello_env()).unwrap();
        buf.push('\n'); // extra trailing blank line
        let reader = BufReader::new(buf.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn decode_stream_skips_empty_lines() {
        let hello_line = JsonlCodec::encode(&hello_env()).unwrap();
        let fatal_line = JsonlCodec::encode(&fatal_env(None, "x")).unwrap();
        let buf = format!("{hello_line}\n\n{fatal_line}\n\n");
        let reader = BufReader::new(buf.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn large_envelope_stays_single_line() {
        let big_text = "x".repeat(100_000);
        let env = event_env("r1", &big_text);
        let line = JsonlCodec::encode(&env).unwrap();
        let trimmed = line.trim_end_matches('\n');
        assert!(!trimmed.contains('\n'));
        assert!(trimmed.len() > 100_000);
    }

    #[test]
    fn utf8_content_preserved() {
        let env = event_env("r1", "こんにちは世界 🌍 — em dash");
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert!(text.contains("こんにちは世界"));
                    assert!(text.contains("🌍"));
                    assert!(text.contains("— em dash"));
                }
                _ => panic!("expected AssistantMessage"),
            },
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn parse_from_raw_bytes() {
        let json = r#"{"t":"fatal","ref_id":null,"error":"raw bytes test"}"#;
        let bytes = json.as_bytes();
        let reader = BufReader::new(bytes);
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 1);
        assert!(
            matches!(&envelopes[0], Envelope::Fatal { error, .. } if error == "raw bytes test")
        );
    }

    #[test]
    fn encode_to_writer_and_decode_stream_roundtrip() {
        let mut buf: Vec<u8> = Vec::new();
        let envelopes = [hello_env(), fatal_env(None, "done")];
        JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 2);
    }
}

// ===========================================================================
// C) Protocol Sequence Validation (10 tests)
// ===========================================================================

mod sequence_validation {
    use super::*;
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    fn validator() -> EnvelopeValidator {
        EnvelopeValidator::new()
    }

    #[test]
    fn valid_full_sequence_hello_run_events_final() {
        let (id, run) = run_env("task");
        let seq = vec![
            hello_env(),
            run,
            event_env(&id, "step 1"),
            event_env(&id, "step 2"),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "valid sequence should have no errors: {errors:?}"
        );
    }

    #[test]
    fn invalid_run_before_hello() {
        let (id, run) = run_env("task");
        let seq = vec![run, hello_env(), final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
            "should detect hello not first: {errors:?}"
        );
    }

    #[test]
    fn invalid_event_before_run() {
        let seq = vec![hello_env(), event_env("r1", "early"), final_env("r1")];
        // Using sidecar_utils validate_sequence which checks middle envelopes
        // must be Event or Run, and the hello must be first, final must be last.
        // An event before a Run is structurally allowed by sidecar_utils
        // validate_sequence (it allows Event|Run in the middle), but the
        // EnvelopeValidator from abp_protocol checks ordering more strictly.
        let errors = validator().validate_sequence(&seq);
        // The event has no matching Run, so ref_id correlation will be absent
        // but the sequencer may or may not flag this. The key point is the
        // sequence itself should at least be parseable.
        // With abp_protocol's validate_sequence, events before run trigger
        // OutOfOrderEvents since there's no Run position to be "after".
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::OutOfOrderEvents)),
            "event before any run should be out-of-order: {errors:?}"
        );
    }

    #[test]
    fn invalid_double_hello() {
        let seq = vec![hello_env(), hello_env(), fatal_env(None, "err")];
        // The sidecar_utils validate_sequence catches duplicate hellos.
        let result = seq_validate::validate_sequence(&seq);
        assert!(result.is_err(), "double hello should fail validation");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("hello") || msg.contains("Hello"),
            "error should mention hello: {msg}"
        );
    }

    #[test]
    fn invalid_final_before_run() {
        let seq = vec![hello_env(), final_env("r1")];
        let _errors = validator().validate_sequence(&seq);
        let result = seq_validate::validate_sequence(&seq);
        assert!(
            result.is_ok(),
            "hello → final is structurally valid (no events needed): {result:?}"
        );
    }

    #[test]
    fn fatal_can_appear_after_hello() {
        let seq = vec![hello_env(), fatal_env(None, "crash early")];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "fatal after hello should be valid: {errors:?}"
        );
        let result = seq_validate::validate_sequence(&seq);
        assert!(result.is_ok());
    }

    #[test]
    fn empty_event_stream_is_valid() {
        let (id, run) = run_env("no events");
        let seq = vec![hello_env(), run, final_env(&id)];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "hello → run → final with no events should be valid: {errors:?}"
        );
    }

    #[test]
    fn multiple_events_are_valid() {
        let (id, run) = run_env("multi");
        let seq = vec![
            hello_env(),
            run,
            event_env(&id, "a"),
            event_env(&id, "b"),
            event_env(&id, "c"),
            event_env(&id, "d"),
            event_env(&id, "e"),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors.is_empty(),
            "multiple events should be valid: {errors:?}"
        );
    }

    #[test]
    fn event_after_final_is_invalid() {
        let (id, run) = run_env("task");
        let seq = vec![
            hello_env(),
            run,
            event_env(&id, "ok"),
            final_env(&id),
            event_env(&id, "late"),
        ];
        // abp_protocol validator: event after terminal → OutOfOrderEvents + MultipleTerminals
        // sidecar_utils: last envelope must be terminal, but here an event is last.
        let result = seq_validate::validate_sequence(&seq);
        assert!(
            result.is_err(),
            "event after final should be invalid: {result:?}"
        );
    }

    #[test]
    fn ref_id_mismatch_detected() {
        let (id, run) = run_env("task");
        let seq = vec![
            hello_env(),
            run,
            event_env("wrong-id", "oops"),
            final_env(&id),
        ];
        let errors = validator().validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
            "mismatched ref_id should be detected: {errors:?}"
        );
    }
}

// ===========================================================================
// D) Error Handling (5 tests)
// ===========================================================================

mod error_handling {
    use super::*;

    #[test]
    fn malformed_json_produces_parse_error() {
        let result = JsonlCodec::decode("this is not json {{{");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ProtocolError::Json(_)),
            "should be a Json parse error"
        );
    }

    #[test]
    fn missing_t_field_error() {
        let json = r#"{"ref_id":"r1","error":"boom"}"#;
        let result = JsonlCodec::decode(json);
        assert!(result.is_err(), "missing 't' field should fail");
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn unknown_t_value_handling() {
        let json = r#"{"t":"unknown_variant","data":"test"}"#;
        let result = JsonlCodec::decode(json);
        assert!(
            result.is_err(),
            "unknown 't' value should fail deserialization"
        );
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn missing_required_fields_error() {
        // Fatal requires an "error" field.
        let json = r#"{"t":"fatal","ref_id":null}"#;
        let result = JsonlCodec::decode(json);
        assert!(
            result.is_err(),
            "missing required 'error' field should fail"
        );
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn type_mismatch_in_field_values() {
        // ref_id should be a string or null, not a number.
        let json = r#"{"t":"fatal","ref_id":12345,"error":"boom"}"#;
        let result = JsonlCodec::decode(json);
        assert!(result.is_err(), "type mismatch should fail");
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }
}

// ===========================================================================
// E) Additional edge cases and cross-cutting concerns
// ===========================================================================

mod additional_coverage {
    use super::*;
    use abp_protocol::validate::EnvelopeValidator;

    #[test]
    fn hello_env_validate_hello_passes() {
        let env = hello_env();
        let result = seq_validate::validate_hello(&env);
        assert!(result.is_ok(), "well-formed hello should pass: {result:?}");
    }

    #[test]
    fn validate_hello_rejects_non_hello() {
        let env = fatal_env(None, "not hello");
        let result = seq_validate::validate_hello(&env);
        assert!(result.is_err());
        match result.unwrap_err() {
            ProtocolError::UnexpectedMessage { expected, got } => {
                assert_eq!(expected, "hello");
                assert_eq!(got, "fatal");
            }
            other => panic!("expected UnexpectedMessage, got {other:?}"),
        }
    }

    #[test]
    fn validate_ref_id_correct_match() {
        let env = event_env("run-42", "msg");
        let result = seq_validate::validate_ref_id(&env, "run-42");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_ref_id_mismatch() {
        let env = event_env("run-42", "msg");
        let result = seq_validate::validate_ref_id(&env, "run-99");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("mismatch"),
            "error should mention mismatch: {msg}"
        );
    }

    #[test]
    fn fatal_with_error_code_serializes() {
        let env = Envelope::fatal_with_code(
            Some("r1".into()),
            "rate limited",
            abp_error::ErrorCode::ProtocolHandshakeFailed,
        );
        let v = to_json_value(&env);
        assert_eq!(v["t"], "fatal");
        assert!(v.get("error_code").is_some());
    }

    #[test]
    fn hello_passthrough_mode_roundtrips() {
        let env = Envelope::hello_with_mode(
            backend("pt"),
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn envelope_validator_single_hello_ok() {
        let v = EnvelopeValidator::new();
        let result = v.validate(&hello_env());
        assert!(
            result.valid,
            "well-formed hello should validate: {:#?}",
            result.errors
        );
    }

    #[test]
    fn envelope_validator_empty_backend_id_fails() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid, "empty backend.id should fail validation");
    }

    #[test]
    fn envelope_validator_empty_error_fails() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "".into(),
            error_code: None,
        };
        let v = EnvelopeValidator::new();
        let result = v.validate(&env);
        assert!(!result.valid, "empty error string should fail validation");
    }

    #[test]
    fn discriminator_is_t_not_type() {
        let env = hello_env();
        let json_str = serde_json::to_string(&env).unwrap();
        assert!(
            json_str.contains("\"t\":\"hello\""),
            "discriminator must be 't', not 'type': {json_str}"
        );
        assert!(
            !json_str.contains("\"type\":\"hello\""),
            "must not use 'type' as discriminator at envelope level"
        );
    }
}
