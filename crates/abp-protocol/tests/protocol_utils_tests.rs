// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for protocol utilities: EnvelopeBuilder, EnvelopeValidator,
//! StreamParser, roundtrip invariants, and error conditions.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::builder::{BuilderError, EnvelopeBuilder};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    }
}

fn sample_receipt() -> abp_core::Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
}

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("do something").build()
}

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn make_run(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: sample_work_order(),
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: sample_event(),
    }
}

fn make_final(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: sample_receipt(),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(Into::into),
        error: error.into(),
    }
}

// ===========================================================================
// EnvelopeBuilder tests
// ===========================================================================

#[test]
fn builder_hello_minimal() {
    let env = EnvelopeBuilder::hello().backend("s1").build().unwrap();
    match env {
        Envelope::Hello {
            backend,
            contract_version,
            mode,
            ..
        } => {
            assert_eq!(backend.id, "s1");
            assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_all_fields() {
    let env = EnvelopeBuilder::hello()
        .backend("sidecar")
        .version("2.0")
        .adapter_version("1.5")
        .mode(ExecutionMode::Passthrough)
        .capabilities(CapabilityManifest::new())
        .build()
        .unwrap();
    match env {
        Envelope::Hello {
            backend,
            mode,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.5"));
            assert_eq!(mode, ExecutionMode::Passthrough);
            assert!(capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_missing_backend() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("backend"));
}

#[test]
fn builder_run_default_id() {
    let wo = sample_work_order();
    let expected_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match env {
        Envelope::Run { id, .. } => assert_eq!(id, expected_id),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_run_custom_ref_id() {
    let env = EnvelopeBuilder::run(sample_work_order())
        .ref_id("custom-id")
        .build()
        .unwrap();
    match env {
        Envelope::Run { id, .. } => assert_eq!(id, "custom-id"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn builder_event_with_ref_id() {
    let env = EnvelopeBuilder::event(sample_event())
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Event { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn builder_event_missing_ref_id() {
    let err = EnvelopeBuilder::event(sample_event()).build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

#[test]
fn builder_final_with_ref_id() {
    let env = EnvelopeBuilder::final_receipt(sample_receipt())
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Final { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn builder_final_missing_ref_id() {
    let err = EnvelopeBuilder::final_receipt(sample_receipt())
        .build()
        .unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

#[test]
fn builder_fatal_no_ref_id() {
    let env = EnvelopeBuilder::fatal("crash").build().unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn builder_fatal_with_ref_id_and_code() {
    let env = EnvelopeBuilder::fatal("oom")
        .ref_id("run-1")
        .code("E_OOM")
        .build()
        .unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "oom");
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// EnvelopeValidator tests
// ===========================================================================

#[test]
fn validator_valid_full_sequence() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("r1"),
        make_event("r1"),
        make_final("r1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn validator_missing_hello() {
    let v = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_event("r1"), make_final("r1")];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn validator_wrong_ref_id() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_run("r1"),
        make_event("wrong"),
        make_final("r1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::RefIdMismatch {
        expected: "r1".into(),
        found: "wrong".into(),
    }));
}

#[test]
fn validator_duplicate_hello_detected_as_not_first() {
    // Second hello would appear after the first — validate_sequence checks
    // that hello is position 0 and that there's at most one terminal.
    // Two hellos means the second is just ignored for position checks, but
    // having a hello at index 0 passes. The protocol doesn't explicitly
    // forbid multiple hellos but the sequence validator checks ordering.
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        make_hello(), // duplicate
        make_run("r1"),
        make_final("r1"),
    ];
    // Should pass since first envelope is Hello
    let errors = v.validate_sequence(&seq);
    // No HelloNotFirst error because position 0 is Hello
    assert!(
        !errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn validator_hello_not_first() {
    let v = EnvelopeValidator::new();
    let seq = vec![make_run("r1"), make_hello(), make_final("r1")];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::HelloNotFirst { position: 1 }));
}

#[test]
fn validator_empty_sequence() {
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn validator_single_envelope_hello_valid() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&make_hello());
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn validator_fatal_with_empty_error_is_invalid() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: String::new(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
}

// ===========================================================================
// StreamParser tests
// ===========================================================================

#[test]
fn stream_parser_single_complete_line() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_push() {
    let mut parser = StreamParser::new();
    let line1 = JsonlCodec::encode(&make_fatal(None, "a")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal(None, "b")).unwrap();
    let input = format!("{line1}{line2}");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn stream_parser_partial_line_buffering() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "partial")).unwrap();
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    // First half — no complete line yet.
    let r1 = parser.push(&bytes[..mid]);
    assert!(r1.is_empty());
    assert!(!parser.is_empty());
    assert_eq!(parser.buffered_len(), mid);

    // Second half — completes the line.
    let r2 = parser.push(&bytes[mid..]);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_empty_lines_skipped() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "x")).unwrap();
    let input = format!("\n\n  \n{line}\n\n");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_finish_flushes_unterminated() {
    let mut parser = StreamParser::new();
    let json = r#"{"t":"fatal","ref_id":null,"error":"noterm"}"#;
    // Push without trailing newline.
    let r1 = parser.push(json.as_bytes());
    assert!(r1.is_empty());

    let r2 = parser.finish();
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_reset_discards_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data without newline");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_malformed_json_returns_error() {
    let mut parser = StreamParser::new();
    let results = parser.push(b"not valid json\n");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_invalid_utf8_returns_error() {
    let mut parser = StreamParser::new();
    // Invalid UTF-8 sequence followed by newline.
    let data = [0xFF, 0xFE, b'\n'];
    let results = parser.push(&data);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_max_line_len_exceeded() {
    let mut parser = StreamParser::with_max_line_len(50);
    let long_line = format!("{}\"long\"\n", "x".repeat(100));
    let results = parser.push(long_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    let err_msg = format!("{}", results[0].as_ref().unwrap_err());
    assert!(err_msg.contains("exceeds maximum"));
}

#[test]
fn stream_parser_byte_at_a_time() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal(None, "byte")).unwrap();
    let bytes = line.as_bytes();

    let mut total_results = Vec::new();
    for &b in bytes {
        total_results.extend(parser.push(&[b]));
    }
    assert_eq!(total_results.len(), 1);
    assert!(total_results[0].is_ok());
}

#[test]
fn stream_parser_unicode_content() {
    let mut parser = StreamParser::new();
    let env = Envelope::Fatal {
        ref_id: None,
        error: "日本語 🚀 émojis".into(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    let decoded = results.into_iter().next().unwrap().unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "日本語 🚀 émojis"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn stream_parser_large_payload() {
    let mut parser = StreamParser::new();
    let big = "x".repeat(500_000);
    let env = Envelope::Fatal {
        ref_id: Some("big".into()),
        error: big.clone(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    let decoded = results.into_iter().next().unwrap().unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 500_000),
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// Protocol invariants: roundtrip through JSON
// ===========================================================================

#[test]
fn roundtrip_hello() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn roundtrip_run() {
    let env = make_run("rt-run");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { id, .. } => assert_eq!(id, "rt-run"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn roundtrip_event() {
    let env = make_event("rt-ev");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { ref_id, .. } if ref_id == "rt-ev"));
}

#[test]
fn roundtrip_final() {
    let env = make_final("rt-fin");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { ref_id, .. } if ref_id == "rt-fin"));
}

#[test]
fn roundtrip_fatal() {
    let env = make_fatal(Some("rt-fat"), "err");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("rt-fat"));
            assert_eq!(error, "err");
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// Error conditions
// ===========================================================================

#[test]
fn decode_malformed_json() {
    let result = JsonlCodec::decode("{not json}");
    assert!(result.is_err());
}

#[test]
fn decode_missing_discriminator() {
    // Valid JSON but missing the "t" tag.
    let result = JsonlCodec::decode(r#"{"error":"boom"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_unknown_envelope_type() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#);
    assert!(result.is_err());
}

#[test]
fn decode_missing_required_fields() {
    // "event" envelope missing "event" field.
    let result = JsonlCodec::decode(r#"{"t":"event","ref_id":"r1"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}
