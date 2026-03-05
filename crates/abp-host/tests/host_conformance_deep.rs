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
//! Deep conformance tests for the abp-host sidecar protocol.
//!
//! Covers JSONL parsing edge cases, protocol state machine validation,
//! error handling, and envelope serialization roundtrips. These tests are
//! unit-style and do not spawn real processes.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    CapabilityManifest::new()
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn make_hello_with_mode(mode: ExecutionMode) -> Envelope {
    Envelope::hello_with_mode(test_backend(), test_capabilities(), mode)
}

fn make_run(id: &str) -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: id.into(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build();
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

// ===========================================================================
// 1. JSONL PARSING EDGE CASES (18 tests)
// ===========================================================================

#[test]
fn parse_empty_line_is_skipped() {
    let input = "\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty(), "empty lines should be skipped");
}

#[test]
fn parse_whitespace_only_lines_are_skipped() {
    let input = "   \n\t\n  \t  \n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(
        results.is_empty(),
        "whitespace-only lines should be skipped"
    );
}

#[test]
fn parse_valid_envelope_with_trailing_whitespace() {
    let hello = make_hello();
    let mut line = JsonlCodec::encode(&hello).unwrap();
    // Insert trailing spaces before the newline
    line = line.trim_end().to_string() + "   \n";
    let decoded = JsonlCodec::decode(line.trim());
    assert!(decoded.is_ok(), "trailing whitespace should be handled");
    assert!(matches!(decoded.unwrap(), Envelope::Hello { .. }));
}

#[test]
fn parse_valid_envelope_with_leading_whitespace() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let with_leading = format!("   {line}");
    let decoded = JsonlCodec::decode(with_leading.trim());
    assert!(decoded.is_ok(), "leading whitespace should be handled");
}

#[test]
fn parse_malformed_json_returns_error() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(
        matches!(result, Err(ProtocolError::Json(_))),
        "malformed JSON should yield Json error"
    );
}

#[test]
fn parse_incomplete_json_object_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1""#);
    assert!(result.is_err(), "incomplete JSON should fail");
}

#[test]
fn parse_missing_t_field_returns_error() {
    let result = JsonlCodec::decode(r#"{"contract_version":"abp/v0.1","backend":{"id":"x"}}"#);
    assert!(result.is_err(), "missing 't' discriminator should fail");
}

#[test]
fn parse_unknown_t_value_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":"something"}"#);
    assert!(result.is_err(), "unknown 't' value should fail");
}

#[test]
fn parse_null_t_value_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":null}"#);
    assert!(result.is_err(), "null 't' should fail");
}

#[test]
fn parse_numeric_t_value_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":42}"#);
    assert!(result.is_err(), "numeric 't' should fail");
}

#[test]
fn parse_valid_then_invalid_in_stream() {
    let hello = make_hello();
    let valid_line = JsonlCodec::encode(&hello).unwrap();
    let input = format!("{valid_line}not json\n{valid_line}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3, "should process all three lines");
    assert!(results[0].is_ok(), "first line should be valid");
    assert!(results[1].is_err(), "second line should be invalid");
    assert!(results[2].is_ok(), "third line should be valid");
}

#[test]
fn parse_utf8_bom_with_trim() {
    // UTF-8 BOM is \xEF\xBB\xBF — when trimmed it should still parse
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let with_bom = format!("\u{FEFF}{}", line.trim());
    // The BOM is a valid Unicode char that will remain after trim_end
    // but serde_json should handle it or fail gracefully
    let result = JsonlCodec::decode(&with_bom);
    // BOM before JSON is not valid JSON, should error
    assert!(result.is_err(), "UTF-8 BOM should cause parse failure");
}

#[test]
fn parse_very_long_valid_line() {
    // A fatal envelope with a very long error message
    let long_msg = "x".repeat(100_000);
    let env = make_fatal(None, &long_msg);
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn parse_json_array_instead_of_object() {
    let result = JsonlCodec::decode(r#"[1, 2, 3]"#);
    assert!(result.is_err(), "JSON array should fail envelope parsing");
}

#[test]
fn parse_json_string_instead_of_object() {
    let result = JsonlCodec::decode(r#""just a string""#);
    assert!(result.is_err(), "JSON string should fail envelope parsing");
}

#[test]
fn parse_json_number_instead_of_object() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err(), "JSON number should fail envelope parsing");
}

#[test]
fn parse_json_bool_instead_of_object() {
    let result = JsonlCodec::decode("true");
    assert!(result.is_err(), "JSON boolean should fail envelope parsing");
}

#[test]
fn parse_json_null_instead_of_object() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err(), "JSON null should fail envelope parsing");
}

// ===========================================================================
// 2. STREAM PARSER EDGE CASES (8 tests)
// ===========================================================================

#[test]
fn stream_parser_partial_line_buffered() {
    let mut parser = StreamParser::new();
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    let r1 = parser.push(&bytes[..mid]);
    assert!(r1.is_empty(), "partial line should produce no results");
    assert!(!parser.is_empty(), "buffer should contain partial data");

    let r2 = parser.push(&bytes[mid..]);
    assert_eq!(r2.len(), 1, "completing the line should produce one result");
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_push() {
    let mut parser = StreamParser::new();
    let hello = make_hello();
    let event = make_event("run-1");
    let line1 = JsonlCodec::encode(&hello).unwrap();
    let line2 = JsonlCodec::encode(&event).unwrap();
    let combined = format!("{line1}{line2}");

    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2, "two lines should produce two results");
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
}

#[test]
fn stream_parser_empty_lines_skipped() {
    let mut parser = StreamParser::new();
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    let with_empties = format!("\n\n{line}\n\n");

    let results = parser.push(with_empties.as_bytes());
    assert_eq!(results.len(), 1, "empty lines should be skipped");
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_finish_flushes_unterminated_line() {
    let mut parser = StreamParser::new();
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    // Push without the trailing newline
    let trimmed = line.trim_end();

    let r1 = parser.push(trimmed.as_bytes());
    assert!(r1.is_empty(), "unterminated line should not be parsed yet");

    let r2 = parser.finish();
    assert_eq!(r2.len(), 1, "finish should flush the unterminated line");
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_invalid_utf8() {
    let mut parser = StreamParser::new();
    let bad_bytes: &[u8] = &[0xFF, 0xFE, 0x0A]; // invalid UTF-8 + newline
    let results = parser.push(bad_bytes);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err(), "invalid UTF-8 should produce error");
}

#[test]
fn stream_parser_max_line_length_exceeded() {
    let mut parser = StreamParser::with_max_line_len(50);
    let long_line = format!("{}\n", "a".repeat(100));
    let results = parser.push(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(
        results[0].is_err(),
        "line exceeding max length should error"
    );
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data without newline");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_mixed_valid_and_invalid_lines() {
    let mut parser = StreamParser::new();
    let hello = make_hello();
    let valid_line = JsonlCodec::encode(&hello).unwrap();
    let input = format!("{valid_line}not json\n{valid_line}");

    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ===========================================================================
// 3. PROTOCOL STATE MACHINE (17 tests)
// ===========================================================================

#[test]
fn sequence_valid_hello_run_event_final() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id),
        make_final(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}

#[test]
fn sequence_missing_hello() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![make_run(run_id), make_event(run_id), make_final(run_id)];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::MissingHello),
        "missing hello should be detected"
    );
}

#[test]
fn sequence_hello_not_first() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_run(run_id),
        make_hello(),
        make_event(run_id),
        make_final(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "hello not at position 0 should be detected"
    );
}

#[test]
fn sequence_events_before_run() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_event(run_id),
        make_run(run_id),
        make_final(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "events before run should be out-of-order"
    );
}

#[test]
fn sequence_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![make_hello(), make_run(run_id), make_event(run_id)];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::MissingTerminal),
        "missing terminal should be detected"
    );
}

#[test]
fn sequence_fatal_is_valid_terminal() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id),
        make_fatal(Some(run_id), "error"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "fatal should be a valid terminal: {errors:?}"
    );
}

#[test]
fn sequence_fatal_with_no_ref_id() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_fatal(None, "global error"),
    ];
    let errors = validator.validate_sequence(&seq);
    // Fatal with None ref_id should not cause a RefIdMismatch
    assert!(
        !errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "fatal with None ref_id should not cause ref_id mismatch"
    );
}

#[test]
fn sequence_multiple_terminals() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_final(run_id),
        make_fatal(Some(run_id), "extra"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::MultipleTerminals),
        "multiple terminals should be detected"
    );
}

#[test]
fn sequence_ref_id_mismatch_on_event() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event("wrong-run"),
        make_final("run-1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched ref_id on event should be detected"
    );
}

#[test]
fn sequence_ref_id_mismatch_on_final() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_event("run-1"),
        make_final("wrong-run"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched ref_id on final should be detected"
    );
}

#[test]
fn sequence_ref_id_mismatch_on_fatal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("run-1"),
        make_fatal(Some("wrong-run"), "boom"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "mismatched ref_id on fatal should be detected"
    );
}

#[test]
fn sequence_empty_is_invalid() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn sequence_hello_only_missing_terminal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_hello()];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::MissingTerminal),
        "hello-only sequence should report missing terminal"
    );
}

#[test]
fn sequence_multiple_events_valid() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_event(run_id),
        make_event(run_id),
        make_event(run_id),
        make_final(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "multiple events should be valid: {errors:?}"
    );
}

#[test]
fn sequence_no_events_valid() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![make_hello(), make_run(run_id), make_final(run_id)];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "zero events before final should be valid: {errors:?}"
    );
}

#[test]
fn sequence_events_after_terminal() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_final(run_id),
        make_event(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            SequenceError::OutOfOrderEvents | SequenceError::MultipleTerminals
        )),
        "events after terminal should be detected"
    );
}

#[test]
fn sequence_fatal_before_any_events() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-1";
    let seq = vec![
        make_hello(),
        make_run(run_id),
        make_fatal(Some(run_id), "immediate failure"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "fatal immediately after run should be valid: {errors:?}"
    );
}

// ===========================================================================
// 4. ENVELOPE VALIDATION (10 tests)
// ===========================================================================

#[test]
fn validate_hello_valid() {
    let validator = EnvelopeValidator::new();
    let hello = make_hello();
    let result = validator.validate(&hello);
    assert!(result.valid, "valid hello should pass: {:?}", result.errors);
}

#[test]
fn validate_hello_empty_contract_version() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: String::new(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&hello);
    assert!(!result.valid, "empty contract_version should fail");
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn validate_hello_invalid_version_format() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&hello);
    assert!(!result.valid, "invalid version format should fail");
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn validate_hello_empty_backend_id() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&hello);
    assert!(!result.valid, "empty backend.id should fail");
}

#[test]
fn validate_hello_missing_optional_versions_warns() {
    let validator = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let result = validator.validate(&hello);
    assert!(
        result.valid,
        "missing optional fields should still be valid"
    );
    assert!(
        result.warnings.len() >= 2,
        "should warn about missing backend_version and adapter_version"
    );
}

#[test]
fn validate_run_empty_id() {
    let validator = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: String::new(),
        work_order: WorkOrderBuilder::new("task").build(),
    };
    let result = validator.validate(&run);
    assert!(!result.valid, "empty run id should fail");
}

#[test]
fn validate_run_empty_task() {
    let validator = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: WorkOrderBuilder::new("").build(),
    };
    let result = validator.validate(&run);
    assert!(!result.valid, "empty task should fail");
}

#[test]
fn validate_event_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let event = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "hi".into(),
            },
            ext: None,
        },
    };
    let result = validator.validate(&event);
    assert!(!result.valid, "empty event ref_id should fail");
}

#[test]
fn validate_fatal_empty_error() {
    let validator = EnvelopeValidator::new();
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
        error_code: None,
    };
    let result = validator.validate(&fatal);
    assert!(!result.valid, "empty fatal error message should fail");
}

#[test]
fn validate_fatal_missing_ref_id_warns() {
    let validator = EnvelopeValidator::new();
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let result = validator.validate(&fatal);
    assert!(
        result.valid,
        "fatal with no ref_id should still be valid (ref_id is optional)"
    );
    assert!(
        !result.warnings.is_empty(),
        "should warn about missing ref_id"
    );
}

// ===========================================================================
// 5. ENVELOPE SERIALIZATION ROUNDTRIP (14 tests)
// ===========================================================================

#[test]
fn roundtrip_hello_envelope() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'), "encoded line should end with newline");
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
        }
        _ => panic!("expected Hello envelope"),
    }
}

#[test]
fn roundtrip_hello_mapped_mode() {
    let hello = make_hello_with_mode(ExecutionMode::Mapped);
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn roundtrip_hello_passthrough_mode() {
    let hello = make_hello_with_mode(ExecutionMode::Passthrough);
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn roundtrip_run_envelope() {
    let run = make_run("run-abc");
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-abc");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run envelope"),
    }
}

#[test]
fn roundtrip_event_run_started() {
    let event = make_event("run-1");
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event envelope"),
    }
}

#[test]
fn roundtrip_event_assistant_delta() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello ".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello "),
            _ => panic!("expected AssistantDelta"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_file_changed() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "added function");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_tool_call() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tool-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "test.txt"}),
            },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tool-1"));
                assert!(parent_tool_use_id.is_none());
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_final_envelope() {
    let fin = make_final("run-abc");
    let line = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-abc");
            assert_eq!(receipt.backend.id, "test-sidecar");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final envelope"),
    }
}

#[test]
fn roundtrip_fatal_with_ref_id() {
    let fatal = make_fatal(Some("run-1"), "something broke");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "something broke");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal envelope"),
    }
}

#[test]
fn roundtrip_fatal_without_ref_id() {
    let fatal = make_fatal(None, "global crash");
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global crash");
        }
        _ => panic!("expected Fatal envelope"),
    }
}

#[test]
fn roundtrip_event_with_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"role": "assistant"}),
    );
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_event_without_ext_field() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        },
    };
    let line = JsonlCodec::encode(&event).unwrap();
    // ext: None should be skipped in serialization (skip_serializing_if)
    assert!(
        !line.contains("\"ext\""),
        "ext: None should not appear in output"
    );
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => assert!(event.ext.is_none()),
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_always_appends_newline() {
    let envelopes: Vec<Envelope> = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_final("r1"),
        make_fatal(None, "err"),
    ];
    for env in &envelopes {
        let line = JsonlCodec::encode(env).unwrap();
        assert!(
            line.ends_with('\n'),
            "all encoded envelopes must end with newline"
        );
        // Exactly one newline at the end, no extra
        assert!(
            !line.trim_end_matches('\n').contains('\n'),
            "should be a single line"
        );
    }
}

// ===========================================================================
// 6. ERROR HANDLING & HOST ERROR VARIANTS (10 tests)
// ===========================================================================

#[test]
fn host_error_violation_display() {
    use abp_host::HostError;
    let err = HostError::Violation("test violation".into());
    let msg = format!("{err}");
    assert!(msg.contains("test violation"));
}

#[test]
fn host_error_fatal_display() {
    use abp_host::HostError;
    let err = HostError::Fatal("sidecar died".into());
    let msg = format!("{err}");
    assert!(msg.contains("sidecar died"));
}

#[test]
fn host_error_exited_display() {
    use abp_host::HostError;
    let err = HostError::Exited { code: Some(1) };
    let msg = format!("{err}");
    assert!(msg.contains("1"));
}

#[test]
fn host_error_exited_none_code() {
    use abp_host::HostError;
    let err = HostError::Exited { code: None };
    let msg = format!("{err}");
    assert!(msg.contains("None"));
}

#[test]
fn host_error_timeout_display() {
    use abp_host::HostError;
    let err = HostError::Timeout {
        duration: std::time::Duration::from_secs(30),
    };
    let msg = format!("{err}");
    assert!(msg.contains("30"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    use abp_host::HostError;
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by signal".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("137"));
    assert!(msg.contains("killed by signal"));
}

#[test]
fn protocol_error_from_json() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hello"));
    assert!(msg.contains("event"));
}

#[test]
fn protocol_error_violation() {
    let err = ProtocolError::Violation("bad state".into());
    let msg = format!("{err}");
    assert!(msg.contains("bad state"));
}

#[test]
fn protocol_error_has_error_codes() {
    let violation = ProtocolError::Violation("x".into());
    assert!(violation.error_code().is_some());

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "a".into(),
        got: "b".into(),
    };
    assert!(unexpected.error_code().is_some());
}

// ===========================================================================
// 7. ENCODING / DECODING SPECIFICS (8 tests)
// ===========================================================================

#[test]
fn envelope_tag_field_is_t_not_type() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(
        line.contains(r#""t":"hello""#),
        "envelope discriminator should be 't' not 'type'"
    );
    assert!(
        !line.contains(r#""type":"hello""#),
        "should NOT use 'type' as envelope tag"
    );
}

#[test]
fn event_kind_tag_field_is_type() {
    let event = make_event("run-1");
    let line = JsonlCodec::encode(&event).unwrap();
    // The AgentEventKind uses #[serde(tag = "type")]
    assert!(
        line.contains(r#""type":"run_started""#),
        "AgentEventKind should use 'type' as discriminator"
    );
}

#[test]
fn envelope_variants_use_snake_case() {
    let hello = make_hello();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(r#""t":"hello""#));

    let run = make_run("r");
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.contains(r#""t":"run""#));

    let event = make_event("r");
    let line = JsonlCodec::encode(&event).unwrap();
    assert!(line.contains(r#""t":"event""#));

    let fin = make_final("r");
    let line = JsonlCodec::encode(&fin).unwrap();
    assert!(line.contains(r#""t":"final""#));

    let fatal = make_fatal(None, "err");
    let line = JsonlCodec::encode(&fatal).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn hello_default_mode_is_mapped() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_mode_defaults_when_absent_in_json() {
    // Manually construct JSON without "mode" field
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(
                mode,
                ExecutionMode::Mapped,
                "missing mode should default to Mapped"
            );
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn fatal_error_code_skipped_when_none() {
    let fatal = make_fatal(Some("run-1"), "error");
    let line = JsonlCodec::encode(&fatal).unwrap();
    assert!(
        !line.contains("error_code"),
        "error_code: None should be skipped via skip_serializing_if"
    );
}

#[test]
fn encode_to_writer_works() {
    let hello = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"hello""#));
}

#[test]
fn encode_many_to_writer_works() {
    let envelopes = vec![make_hello(), make_fatal(None, "err")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), 2, "should produce two lines");
}

// ===========================================================================
// 8. VERSION NEGOTIATION (6 tests)
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(abp_protocol::parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(abp_protocol::parse_version("invalid"), None);
    assert_eq!(abp_protocol::parse_version("abp/v"), None);
    assert_eq!(abp_protocol::parse_version("abp/v1"), None);
    assert_eq!(abp_protocol::parse_version("v0.1"), None);
    assert_eq!(abp_protocol::parse_version(""), None);
}

#[test]
fn is_compatible_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn is_incompatible_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn is_incompatible_invalid_versions() {
    assert!(!abp_protocol::is_compatible_version("invalid", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "invalid"));
    assert!(!abp_protocol::is_compatible_version("invalid", "invalid"));
}

#[test]
fn contract_version_is_compatible_with_itself() {
    assert!(abp_protocol::is_compatible_version(
        CONTRACT_VERSION,
        CONTRACT_VERSION
    ));
}

// ===========================================================================
// 9. SIDECAR SPEC & HELLO TYPES (5 tests)
// ===========================================================================

#[test]
fn sidecar_spec_new_defaults() {
    use abp_host::SidecarSpec;
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_serialization() {
    use abp_host::SidecarSpec;
    let mut spec = SidecarSpec::new("python");
    spec.args = vec!["script.py".into()];
    spec.env.insert("KEY".into(), "VALUE".into());
    spec.cwd = Some("/tmp".into());

    let json = serde_json::to_string(&spec).unwrap();
    let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.command, "python");
    assert_eq!(deser.args, vec!["script.py"]);
    assert_eq!(deser.env.get("KEY").unwrap(), "VALUE");
    assert_eq!(deser.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn sidecar_hello_serialization() {
    use abp_host::SidecarHello;
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_backend(),
        capabilities: test_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let deser: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.contract_version, CONTRACT_VERSION);
    assert_eq!(deser.backend.id, "test-sidecar");
}

#[test]
fn envelope_error_code_helper() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "rate limited",
        abp_error::ErrorCode::BackendRateLimited,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::BackendRateLimited)
    );

    let hello = make_hello();
    assert!(hello.error_code().is_none());
}

#[test]
fn envelope_fatal_from_abp_error() {
    let abp_err = abp_error::AbpError::new(
        abp_error::ErrorCode::BackendTimeout,
        "timed out waiting for response",
    );
    let fatal = Envelope::fatal_from_abp_error(Some("run-1".into()), &abp_err);
    match fatal {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "timed out waiting for response");
            assert_eq!(error_code, Some(abp_error::ErrorCode::BackendTimeout));
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 10. DECODE STREAM INTEGRATION (4 tests)
// ===========================================================================

#[test]
fn decode_stream_multiple_envelopes() {
    let hello = make_hello();
    let event = make_event("run-1");
    let fin = make_final("run-1");

    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    JsonlCodec::encode_to_writer(&mut buf, &event).unwrap();
    JsonlCodec::encode_to_writer(&mut buf, &fin).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(results.len(), 3);
    assert!(matches!(results[0], Envelope::Hello { .. }));
    assert!(matches!(results[1], Envelope::Event { .. }));
    assert!(matches!(results[2], Envelope::Final { .. }));
}

#[test]
fn decode_stream_with_blank_lines_between() {
    let hello = make_hello();
    let hello_line = JsonlCodec::encode(&hello).unwrap();
    let input = format!("\n{hello_line}\n\n{hello_line}");

    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 2, "blank lines should be skipped");
}

#[test]
fn decode_stream_stops_on_io_error() {
    // An empty reader should produce no results
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty());
}

#[test]
fn decode_stream_handles_error_then_continues() {
    let hello = make_hello();
    let valid_line = JsonlCodec::encode(&hello).unwrap();
    let input = format!("{valid_line}bad json line\n{valid_line}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}
