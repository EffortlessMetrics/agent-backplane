// SPDX-License-Identifier: MIT OR Apache-2.0
//! Robustness tests for the JSONL protocol handshake and envelope processing.
//!
//! Covers edge cases: timeouts, corrupt data, encoding issues, large payloads,
//! rapid-fire events, out-of-order messages, and process termination semantics.

use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_event(ref_id: &str, msg: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: msg.into() },
            ext: None,
        },
    }
}

fn make_run_envelope() -> (String, Envelope) {
    let wo = WorkOrderBuilder::new("test task").build();
    let run_id = wo.id.to_string();
    let env = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    (run_id, env)
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(uuid::Uuid::new_v4())
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

// ===========================================================================
// 1. Handshake timeout – sidecar doesn't send hello
// ===========================================================================

/// An empty stream (EOF before any message) should produce no envelopes.
#[test]
fn handshake_timeout_empty_stream() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(
        envelopes.is_empty(),
        "expected no envelopes from empty stream"
    );
}

/// The sequence validator should detect a missing Hello.
#[test]
fn handshake_missing_hello_sequence_validation() {
    let (run_id, run_env) = make_run_envelope();
    let final_env = make_final(&run_id);
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[run_env, final_env]);
    assert!(
        errors.contains(&SequenceError::MissingHello),
        "expected MissingHello error, got: {errors:?}"
    );
}

// ===========================================================================
// 2. Partial hello – incomplete JSON line
// ===========================================================================

#[test]
fn partial_hello_incomplete_json() {
    // A truncated JSON line should fail to decode.
    let partial = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x""#;
    let result = JsonlCodec::decode(partial);
    assert!(result.is_err(), "partial JSON should fail to decode");
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn partial_hello_stream_parser_buffers() {
    let hello = make_hello();
    let full_line = JsonlCodec::encode(&hello).unwrap();
    let bytes = full_line.as_bytes();

    let mut parser = StreamParser::new();

    // Feed only the first half – no complete line yet.
    let mid = bytes.len() / 2;
    let results = parser.push(&bytes[..mid]);
    assert!(results.is_empty(), "partial line should yield no envelopes");

    // Feed the remainder – now we should get one envelope.
    let results = parser.push(&bytes[mid..]);
    assert_eq!(results.len(), 1, "complete line should yield one envelope");
    assert!(results[0].is_ok());
}

// ===========================================================================
// 3. Multiple hello envelopes (protocol violation)
// ===========================================================================

#[test]
fn multiple_hello_envelopes_second_ignored_by_validator() {
    // The sequence validator only checks that the first envelope is Hello.
    // Additional Hello envelopes are not flagged by validate_sequence (the
    // host's run loop ignores them). This test documents that behavior.
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let hello2 = make_hello();
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let sequence = [hello, run_env, hello2, final_env];
    let errors = validator.validate_sequence(&sequence);
    // No errors because hello is first, and duplicate hello is silently skipped.
    // This documents current behavior; a stricter validator could flag this.
    assert!(
        errors.is_empty(),
        "duplicate hello after position 0 is currently tolerated: {errors:?}"
    );
}

#[test]
fn duplicate_hello_as_first_two_messages() {
    // Two hellos followed by run + final: the second hello is not an event
    // so the validator should still pass (hello at pos 0 is fine).
    let hello1 = make_hello();
    let hello2 = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello1, hello2, run_env, final_env]);
    // Validator doesn't specifically flag extra hellos.
    assert!(
        errors.is_empty(),
        "two hellos at start currently tolerated: {errors:?}"
    );
}

#[test]
fn hello_not_first_detected() {
    let (run_id, run_env) = make_run_envelope();
    let hello = make_hello();
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[run_env, hello, final_env]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "hello not at position 0 should be flagged: {errors:?}"
    );
}

// ===========================================================================
// 4. Missing ref_id in event envelopes
// ===========================================================================

#[test]
fn empty_ref_id_event_validation_error() {
    let event = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        },
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&event);
    assert!(
        !result.valid,
        "empty ref_id should cause a validation error"
    );
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "ref_id"
    )));
}

#[test]
fn empty_ref_id_final_validation_error() {
    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(uuid::Uuid::new_v4())
        .build();
    let final_env = Envelope::Final {
        ref_id: String::new(),
        receipt,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&final_env);
    assert!(
        !result.valid,
        "empty ref_id on Final should fail validation"
    );
}

// ===========================================================================
// 5. Out-of-order events (events for wrong ref_id)
// ===========================================================================

#[test]
fn wrong_ref_id_detected_in_sequence() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let wrong_event = make_event("wrong-run-id", "rogue event");
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, wrong_event, final_env]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched ref_id should be flagged: {errors:?}"
    );
}

#[test]
fn event_before_run_is_out_of_order() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let early_event = make_event(&run_id, "too early");
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, early_event, run_env, final_env]);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "event before Run should be out-of-order: {errors:?}"
    );
}

#[test]
fn event_after_final_is_out_of_order() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let final_env = make_final(&run_id);
    let late_event = make_event(&run_id, "too late");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, final_env, late_event]);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "event after Final should be out-of-order: {errors:?}"
    );
}

// ===========================================================================
// 6. Binary/corrupt data on stdout
// ===========================================================================

#[test]
fn binary_garbage_fails_decode() {
    let garbage = "\x00\x01\x02\x7F\x7E";
    let result = JsonlCodec::decode(garbage);
    assert!(result.is_err(), "binary garbage should fail to decode");
}

#[test]
fn corrupt_json_field_name() {
    let corrupt = r#"{"t":"hello","contract_version":null,"backend":123}"#;
    let result = JsonlCodec::decode(corrupt);
    assert!(result.is_err(), "corrupt field types should fail to decode");
}

#[test]
fn unknown_envelope_type() {
    let unknown = r#"{"t":"unknown_type","data":"foo"}"#;
    let result = JsonlCodec::decode(unknown);
    assert!(result.is_err(), "unknown envelope type should fail");
}

#[test]
fn stream_parser_rejects_invalid_utf8() {
    let mut parser = StreamParser::new();
    // Invalid UTF-8 followed by newline.
    let data: &[u8] = &[0x80, 0x81, 0xFF, b'\n'];
    let results = parser.push(data);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err(), "invalid UTF-8 should produce an error");
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => {
            assert!(msg.contains("UTF-8"), "error should mention UTF-8: {msg}");
        }
        other => panic!("expected Violation, got: {other:?}"),
    }
}

#[test]
fn stream_parser_handles_mixed_valid_and_corrupt() {
    let hello = make_hello();
    let hello_line = JsonlCodec::encode(&hello).unwrap();

    let mut parser = StreamParser::new();

    // Feed valid line, then corrupt line, then another valid line.
    let mut input = Vec::new();
    input.extend_from_slice(hello_line.as_bytes());
    input.extend_from_slice(b"NOT JSON\n");
    input.extend_from_slice(hello_line.as_bytes());

    let results = parser.push(&input);
    assert_eq!(results.len(), 3, "should yield 3 results for 3 lines");
    assert!(results[0].is_ok(), "first line should be ok");
    assert!(results[1].is_err(), "corrupt line should be error");
    assert!(results[2].is_ok(), "third line should be ok");
}

// ===========================================================================
// 7. Very large payloads
// ===========================================================================

#[test]
fn large_payload_roundtrip() {
    // Create a ~1MB payload (smaller than 50MB to keep test fast, but tests the path).
    let big_text = "x".repeat(1_000_000);
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: big_text.clone(),
            },
            ext: None,
        },
    };

    let encoded = JsonlCodec::encode(&event).unwrap();
    assert!(encoded.len() > 1_000_000, "encoded should be > 1MB");
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, big_text);
            }
            _ => panic!("wrong event kind"),
        },
        _ => panic!("wrong envelope type"),
    }
}

#[test]
fn large_payload_triggers_validation_warning() {
    // 11 MB message should exceed the 10 MiB recommended payload.
    let big_text = "A".repeat(11 * 1024 * 1024);
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: big_text },
            ext: None,
        },
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&event);
    assert!(
        result.warnings.iter().any(|w| matches!(
            w,
            abp_protocol::validate::ValidationWarning::LargePayload { .. }
        )),
        "large payload should produce a warning: {:?}",
        result.warnings
    );
}

#[test]
fn stream_parser_rejects_line_exceeding_max_len() {
    let mut parser = StreamParser::with_max_line_len(128);
    // Feed a line that exceeds 128 bytes.
    let mut big_line = "x".repeat(256);
    big_line.push('\n');
    let results = parser.push(big_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(
        results[0].is_err(),
        "line exceeding max_line_len should error"
    );
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => {
            assert!(msg.contains("exceeds maximum"), "msg: {msg}");
        }
        other => panic!("expected Violation, got: {other:?}"),
    }
}

// ===========================================================================
// 8. Rapid-fire events (10,000 events/sec throughput)
// ===========================================================================

#[test]
fn rapid_fire_event_encoding_decoding() {
    let count = 10_000;
    let mut encoded_lines = String::new();
    for i in 0..count {
        let event = make_event("run-rapid", &format!("msg-{i}"));
        encoded_lines.push_str(&JsonlCodec::encode(&event).unwrap());
    }

    let reader = BufReader::new(encoded_lines.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("all 10k events should decode successfully");
    assert_eq!(decoded.len(), count, "should decode all {count} events");
}

#[test]
fn rapid_fire_stream_parser() {
    let count = 10_000;
    let mut parser = StreamParser::new();
    let mut total_envelopes = 0;

    for i in 0..count {
        let event = make_event("run-rapid", &format!("msg-{i}"));
        let line = JsonlCodec::encode(&event).unwrap();
        let results = parser.push(line.as_bytes());
        for r in &results {
            assert!(r.is_ok(), "event {i} failed: {:?}", r.as_ref().err());
        }
        total_envelopes += results.len();
    }

    assert_eq!(
        total_envelopes, count,
        "stream parser should yield all {count} envelopes"
    );
}

#[test]
fn rapid_fire_sequence_validation() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();

    let mut sequence = vec![hello, run_env];
    for i in 0..1_000 {
        sequence.push(make_event(&run_id, &format!("msg-{i}")));
    }
    sequence.push(make_final(&run_id));

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors.is_empty(),
        "valid 1000-event sequence should have no errors: {errors:?}"
    );
}

// ===========================================================================
// 9. Graceful vs forceful process termination
// ===========================================================================

/// A properly terminated sequence has Final.
#[test]
fn graceful_termination_final_present() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let event = make_event(&run_id, "working...");
    let final_env = make_final(&run_id);

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, event, final_env]);
    assert!(
        errors.is_empty(),
        "graceful termination should pass: {errors:?}"
    );
}

/// A sequence that ends abruptly (no Final/Fatal) is a protocol violation.
#[test]
fn forceful_termination_missing_terminal() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let event = make_event(&run_id, "working...");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, event]);
    assert!(
        errors.contains(&SequenceError::MissingTerminal),
        "abrupt termination should detect MissingTerminal: {errors:?}"
    );
}

/// A Fatal envelope is a valid (if unhappy) termination.
#[test]
fn fatal_is_valid_termination() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let fatal = Envelope::Fatal {
        ref_id: Some(run_id.clone()),
        error: "out of memory".into(),
        error_code: None,
    };

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, fatal]);
    assert!(
        errors.is_empty(),
        "fatal termination should be valid: {errors:?}"
    );
}

/// Multiple terminals (both Fatal and Final) are flagged.
#[test]
fn multiple_terminals_flagged() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let final_env = make_final(&run_id);
    let fatal = Envelope::Fatal {
        ref_id: Some(run_id.clone()),
        error: "extra".into(),
        error_code: None,
    };

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run_env, final_env, fatal]);
    assert!(
        errors.contains(&SequenceError::MultipleTerminals),
        "multiple terminals should be flagged: {errors:?}"
    );
}

// ===========================================================================
// 10. Mixed encoding (UTF-8 BOM, invalid UTF-8)
// ===========================================================================

#[test]
fn utf8_bom_in_stream() {
    let hello = make_hello();
    let hello_json = JsonlCodec::encode(&hello).unwrap();

    // Prepend UTF-8 BOM (EF BB BF) to the first line.
    let mut input = Vec::new();
    input.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    input.extend_from_slice(hello_json.as_bytes());

    let mut parser = StreamParser::new();
    let results = parser.push(&input);
    assert_eq!(
        results.len(),
        1,
        "BOM-prefixed line should produce a result"
    );
    // The BOM character prepended to valid JSON will likely cause a parse error
    // since the JSON parser doesn't expect a BOM inside the JSON.
    // This is the expected behavior – sidecars should NOT send BOMs.
    assert!(
        results[0].is_err(),
        "UTF-8 BOM-prefixed JSON should fail to parse (BOM is not valid JSON)"
    );
}

#[test]
fn utf8_bom_stripped_manually_succeeds() {
    let hello = make_hello();
    let hello_json = JsonlCodec::encode(&hello).unwrap();

    // Simulate stripping BOM before decoding.
    let line = hello_json.trim();
    let stripped = line.strip_prefix('\u{FEFF}').unwrap_or(line);
    let result = JsonlCodec::decode(stripped);
    assert!(result.is_ok(), "BOM-stripped JSON should decode fine");
}

#[test]
fn invalid_utf8_bytes_in_middle_of_json() {
    let mut parser = StreamParser::new();
    // Start with valid JSON opening, then inject invalid UTF-8, then newline.
    let mut data = Vec::new();
    data.extend_from_slice(b"{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"bad ");
    data.extend_from_slice(&[0xC0, 0xAF]); // overlong UTF-8 encoding (invalid)
    data.extend_from_slice(b"\"}\n");

    let results = parser.push(&data);
    assert_eq!(results.len(), 1);
    assert!(
        results[0].is_err(),
        "invalid UTF-8 in JSON should produce an error"
    );
}

#[test]
fn valid_multibyte_utf8_in_payload() {
    // Ensure legitimate multi-byte UTF-8 (e.g. emoji, CJK) works fine.
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello \u{1F30D} \u{4E16}\u{754C} \u{0645}\u{0631}\u{062D}\u{0628}\u{0627}"
                    .into(),
            },
            ext: None,
        },
    };

    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\u{1F30D}'));
                assert!(text.contains("\u{4E16}\u{754C}"));
            }
            _ => panic!("wrong event kind"),
        },
        _ => panic!("wrong envelope type"),
    }
}

// ===========================================================================
// Additional edge cases
// ===========================================================================

#[test]
fn empty_lines_skipped_in_decode_stream() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    // Intersperse blank lines and whitespace-only lines.
    let input = format!("\n  \n{line}\n\n  \n{line}\n");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2, "blank lines should be skipped");
}

#[test]
fn stream_parser_finish_flushes_incomplete_line() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    // Remove the trailing newline so it's "incomplete".
    let no_newline = line.trim_end_matches('\n');

    let mut parser = StreamParser::new();
    let results = parser.push(no_newline.as_bytes());
    assert!(results.is_empty(), "no newline means no envelope yet");

    let results = parser.finish();
    assert_eq!(
        results.len(),
        1,
        "finish() should flush the incomplete line"
    );
    assert!(results[0].is_ok());
}

#[test]
fn encode_to_writer_roundtrip() {
    let hello = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();

    let line = String::from_utf8(buf).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn encode_many_to_writer_roundtrip() {
    let hello = make_hello();
    let (run_id, run_env) = make_run_envelope();
    let final_env = make_final(&run_id);

    let envelopes = [hello, run_env, final_env];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn fatal_with_error_code_roundtrips() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "something broke",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_carries_error_codes() {
    let violation = ProtocolError::Violation("test".into());
    assert_eq!(
        violation.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        unexpected.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}
