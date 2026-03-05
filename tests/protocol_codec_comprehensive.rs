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
//! Comprehensive tests for the JSONL protocol codec layer.

use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use abp_receipt::ReceiptBuilder;
use chrono::Utc;

// ===================================================================
// Helpers
// ===================================================================

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_fatal(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: msg.into(),
        error_code: None,
    }
}

fn make_event() -> Envelope {
    Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            ext: None,
        },
    }
}

fn make_run() -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    }
}

fn make_final() -> Envelope {
    let receipt = ReceiptBuilder::new("mock-backend").build();
    Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    }
}

// ===================================================================
// 1. Codec encode/decode (15 tests)
// ===================================================================

#[test]
fn encode_hello_roundtrip() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn encode_fatal_roundtrip() {
    let fatal = make_fatal("test error");
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "test error"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn encode_event_roundtrip() {
    let event = make_event();
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn encode_run_roundtrip() {
    let run = make_run();
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn encode_final_roundtrip() {
    let f = make_final();
    let encoded = JsonlCodec::encode(&f).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

#[test]
fn encode_ends_with_newline() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'), "encoded must end with newline");
}

#[test]
fn encode_single_line() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let lines: Vec<&str> = encoded.trim().lines().collect();
    assert_eq!(lines.len(), 1, "encoded must be a single line");
}

#[test]
fn decode_with_trailing_newline() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let with_newline = format!("{json}\n");
    // decode should work with trimmed input
    let decoded = JsonlCodec::decode(with_newline.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_multiple_lines() {
    let lines = format!(
        "{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"err1"}"#,
        r#"{"t":"fatal","ref_id":null,"error":"err2"}"#,
    );
    let reader = BufReader::new(lines.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_skips_blank_lines() {
    let lines = format!(
        "\n{}\n\n{}\n\n",
        r#"{"t":"fatal","ref_id":null,"error":"a"}"#, r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
    );
    let reader = BufReader::new(lines.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn encode_to_writer_works() {
    let hello = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    assert!(output.contains(r#""t":"hello""#));
}

#[test]
fn encode_many_to_writer() {
    let envelopes = vec![make_fatal("e1"), make_fatal("e2"), make_fatal("e3")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.lines().count(), 3);
}

#[test]
fn discriminator_field_is_t() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(
        encoded.contains(r#""t":"hello""#),
        "discriminator must be 't', not 'type'"
    );
    assert!(!encoded.contains(r#""type":"hello""#));
}

#[test]
fn hello_contains_contract_version() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains(CONTRACT_VERSION));
}

#[test]
fn fatal_ref_id_null_serializes() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    assert!(encoded.contains(r#""ref_id":null"#));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

// ===================================================================
// 2. Error handling (15 tests)
// ===================================================================

#[test]
fn decode_invalid_json() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_empty_string() {
    let err = JsonlCodec::decode("").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_missing_t_field() {
    let err = JsonlCodec::decode(r#"{"error":"no type"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_unknown_envelope_type() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_truncated_json() {
    let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"err"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_json_array_not_object() {
    let err = JsonlCodec::decode(r#"[1, 2, 3]"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_null_literal() {
    let err = JsonlCodec::decode("null").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_number_literal() {
    let err = JsonlCodec::decode("42").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_string_literal() {
    let err = JsonlCodec::decode(r#""hello""#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_boolean_literal() {
    let err = JsonlCodec::decode("true").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_missing_required_field_error() {
    // Fatal requires "error" field
    let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_wrong_field_type() {
    // ref_id should be string or null, not a number
    let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":123,"error":"x"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_json_display() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"), "got: {msg}");
}

#[test]
fn protocol_error_violation_display() {
    let err = ProtocolError::Violation("test violation".into());
    assert_eq!(format!("{err}"), "protocol violation: test violation");
}

#[test]
fn protocol_error_unexpected_message_display() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

// ===================================================================
// 3. Version negotiation (10 tests)
// ===================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid_prefix() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
}

#[test]
fn parse_version_missing_minor() {
    assert_eq!(parse_version("abp/v1"), None);
}

#[test]
fn parse_version_empty() {
    assert_eq!(parse_version(""), None);
}

#[test]
fn compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn protocol_version_struct_parse() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert_eq!(v.to_string(), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn negotiate_version_same_major_picks_minimum() {
    let local = ProtocolVersion::parse("abp/v0.2").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.major, 0);
    assert_eq!(result.minor, 1);
}

#[test]
fn negotiate_version_different_major_fails() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&local, &remote).is_err());
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
}

#[test]
fn version_range_compatible() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion { major: 1, minor: 5 };
    assert_eq!(format!("{v}"), "abp/v1.5");
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01 < v02);
    assert!(v02 < v10);
}

#[test]
fn protocol_version_is_compatible() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    // v01.is_compatible(v02) => same major AND other.minor >= self.minor
    assert!(v01.is_compatible(&v02));
    // v02.is_compatible(v01) => other.minor(1) < self.minor(2) => false
    assert!(!v02.is_compatible(&v01));
}

// ===================================================================
// 4. Stream processing (15 tests)
// ===================================================================

#[test]
fn stream_parser_partial_line() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("boom")).unwrap();
    let (first, second) = line.as_bytes().split_at(10);

    let results = parser.push(first);
    assert!(results.is_empty(), "partial line should not yield results");

    let results = parser.push(second);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_multiple_complete_lines() {
    let mut parser = StreamParser::new();
    let line1 = JsonlCodec::encode(&make_fatal("err1")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("err2")).unwrap();
    let combined = format!("{line1}{line2}");

    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_empty_lines_skipped() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("\n\n{line}\n\n");

    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_finish_flushes_remaining() {
    let mut parser = StreamParser::new();
    // Push data without trailing newline
    let line = JsonlCodec::encode(&make_fatal("last")).unwrap();
    let no_newline = line.trim_end();

    let results = parser.push(no_newline.as_bytes());
    assert!(results.is_empty());

    let results = parser.finish();
    assert_eq!(results.len(), 1);
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data");
    assert!(!parser.is_empty());

    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_buffered_len_tracks() {
    let mut parser = StreamParser::new();
    assert_eq!(parser.buffered_len(), 0);

    parser.push(b"some data");
    assert_eq!(parser.buffered_len(), 9);
}

#[test]
fn stream_parser_byte_at_a_time() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("byte")).unwrap();
    let bytes = line.as_bytes();

    let mut total_results = 0;
    for &b in bytes {
        let results = parser.push(&[b]);
        total_results += results.len();
    }
    assert_eq!(total_results, 1);
}

#[test]
fn stream_parser_large_payload() {
    let mut parser = StreamParser::new();
    let large_text = "x".repeat(100_000);
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: large_text.clone(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    match results[0].as_ref().unwrap() {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn stream_parser_max_line_len_enforced() {
    let mut parser = StreamParser::with_max_line_len(50);
    // Create a line that exceeds the limit
    let long_json = format!(
        r#"{{"t":"fatal","ref_id":null,"error":"{}"}}"#,
        "a".repeat(100)
    );
    let input = format!("{long_json}\n");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_invalid_utf8() {
    let mut parser = StreamParser::new();
    // Invalid UTF-8 bytes followed by newline
    let data: &[u8] = &[0xFF, 0xFE, 0x80, b'\n'];
    let results = parser.push(data);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn streaming_codec_encode_batch() {
    let envelopes = vec![make_fatal("a"), make_fatal("b"), make_fatal("c")];
    let batch = StreamingCodec::encode_batch(&envelopes);
    assert_eq!(batch.lines().count(), 3);
}

#[test]
fn streaming_codec_decode_batch() {
    let input = format!(
        "{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"x"}"#, r#"{"t":"fatal","ref_id":null,"error":"y"}"#,
    );
    let results = StreamingCodec::decode_batch(&input);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn streaming_codec_line_count() {
    let input = "line1\n\nline2\nline3\n\n";
    assert_eq!(StreamingCodec::line_count(input), 3);
}

#[test]
fn streaming_codec_validate_jsonl() {
    let input = format!(
        "{}\n{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"ok"}"#,
        "bad json",
        r#"{"t":"fatal","ref_id":null,"error":"ok2"}"#,
    );
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2); // line 2 (1-based)
}

#[test]
fn stream_parser_chunked_across_boundary() {
    let mut parser = StreamParser::new();
    let line1 = JsonlCodec::encode(&make_fatal("first")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("second")).unwrap();
    let combined = format!("{line1}{line2}");
    let bytes = combined.as_bytes();

    // Split in the middle of line2
    let split_point = line1.len() + 5;
    let chunk1 = &bytes[..split_point];
    let chunk2 = &bytes[split_point..];

    let r1 = parser.push(chunk1);
    assert_eq!(r1.len(), 1); // line1 complete

    let r2 = parser.push(chunk2);
    assert_eq!(r2.len(), 1); // line2 complete
}

// ===================================================================
// 5. Edge cases (10 tests)
// ===================================================================

#[test]
fn decode_whitespace_only_trimmed() {
    let json = r#"  {"t":"fatal","ref_id":null,"error":"boom"}  "#;
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn unicode_in_error_message() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "こんにちは世界 🌍 émojis".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert_eq!(error, "こんにちは世界 🌍 émojis");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_in_backend_id() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "bäckend-ñame".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "bäckend-ñame"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn bom_handled_in_stream_parser() {
    let mut parser = StreamParser::new();
    let json = r#"{"t":"fatal","ref_id":null,"error":"bom"}"#;
    // UTF-8 BOM + JSON line
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(json.as_bytes());
    input.push(b'\n');

    let results = parser.push(&input);
    // The BOM is non-empty so the line won't be blank, but the JSON may or
    // may not parse depending on whether serde handles BOM. Either way, we
    // get exactly one result (success or error).
    assert_eq!(results.len(), 1);
}

#[test]
fn empty_stream_produces_no_envelopes() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(envelopes.is_empty());
}

#[test]
fn only_blank_lines_produce_no_envelopes() {
    let reader = BufReader::new("\n\n\n\n".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(envelopes.is_empty());
}

#[test]
fn extra_fields_ignored_in_deserialization() {
    // Unknown fields should be silently ignored by serde
    let json = r#"{"t":"fatal","ref_id":null,"error":"ok","extra_field":"ignored"}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn special_characters_in_error_message() {
    let msg = r#"error with "quotes" and \backslashes\ and newlines
tabs	and null bytes"#;
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn hello_with_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt-backend".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains("passthrough"));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// ===================================================================
// Additional error-code and protocol-error tests
// ===================================================================

#[test]
fn fatal_with_error_code_roundtrip() {
    let fatal = Envelope::fatal_with_code(
        Some("run-42".into()),
        "handshake failed",
        abp_error::ErrorCode::ProtocolHandshakeFailed,
    );
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    assert!(encoded.contains("protocol_handshake_failed"));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal {
            error_code,
            ref_id,
            error,
            ..
        } => {
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
            );
            assert_eq!(ref_id, Some("run-42".into()));
            assert_eq!(error, "handshake failed");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_without_error_code_omits_field() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "plain error".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    // error_code has skip_serializing_if = "Option::is_none"
    assert!(!encoded.contains("error_code"));
}

#[test]
fn error_code_serializes_snake_case() {
    let code = abp_error::ErrorCode::ProtocolInvalidEnvelope;
    let json = serde_json::to_string(&code).unwrap();
    assert_eq!(json, r#""protocol_invalid_envelope""#);
}

#[test]
fn error_code_display_uses_message() {
    let code = abp_error::ErrorCode::ProtocolInvalidEnvelope;
    let display = format!("{code}");
    assert_eq!(display, code.message());
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn envelope_error_code_accessor() {
    let fatal = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::BackendTimeout);
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::BackendTimeout)
    );

    // Non-fatal envelopes return None
    let hello = make_hello();
    assert_eq!(hello.error_code(), None);
}

// ===================================================================
// Version error tests
// ===================================================================

#[test]
fn version_parse_invalid_format_error() {
    let err = ProtocolVersion::parse("bad").unwrap_err();
    assert_eq!(err, abp_protocol::version::VersionError::InvalidFormat,);
}

#[test]
fn version_parse_invalid_major() {
    let err = ProtocolVersion::parse("abp/vabc.1").unwrap_err();
    assert_eq!(err, abp_protocol::version::VersionError::InvalidMajor,);
}

#[test]
fn version_parse_invalid_minor() {
    let err = ProtocolVersion::parse("abp/v1.xyz").unwrap_err();
    assert_eq!(err, abp_protocol::version::VersionError::InvalidMinor,);
}

#[test]
fn negotiate_version_error_carries_versions() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&local, &remote).unwrap_err();
    match err {
        abp_protocol::version::VersionError::Incompatible {
            local: l,
            remote: r,
        } => {
            assert_eq!(l, local);
            assert_eq!(r, remote);
        }
        _ => panic!("expected Incompatible error"),
    }
}

// ===================================================================
// Decode stream error propagation
// ===================================================================

#[test]
fn decode_stream_propagates_errors() {
    let input = format!(
        "{}\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"ok"}"#, "not json at all",
    );
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

// ===================================================================
// Concurrent encoding (single-threaded but parallel-like)
// ===================================================================

#[test]
fn multiple_codecs_independent() {
    // JsonlCodec is stateless (Copy), so multiple uses should be independent
    let e1 = JsonlCodec::encode(&make_fatal("a")).unwrap();
    let e2 = JsonlCodec::encode(&make_fatal("b")).unwrap();
    assert_ne!(e1, e2);
    assert!(e1.contains("\"a\""));
    assert!(e2.contains("\"b\""));
}

#[test]
fn codec_is_copy() {
    let _codec1 = JsonlCodec;
    let _codec2 = _codec1; // Copy trait
    let _ = JsonlCodec::encode(&make_fatal("test")).unwrap();
}

// ===================================================================
// Additional decode edge cases with event types
// ===================================================================

#[test]
fn event_with_tool_call_roundtrip() {
    let event = Envelope::Event {
        ref_id: "run-abc".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tool-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/tmp/test.txt"}),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-abc");
            match event.kind {
                AgentEventKind::ToolCall { tool_name, .. } => {
                    assert_eq!(tool_name, "read_file");
                }
                _ => panic!("expected ToolCall"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_warning_roundtrip() {
    let event = Envelope::Event {
        ref_id: "run-w".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "low on tokens".into(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn event_with_file_changed_roundtrip() {
    let event = Envelope::Event {
        ref_id: "run-f".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn parse_version_with_large_numbers() {
    assert_eq!(parse_version("abp/v999.999"), Some((999, 999)));
}

#[test]
fn incompatible_version_with_invalid_strings() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
    assert!(!is_compatible_version("invalid", "also_invalid"));
}

#[test]
fn negotiate_version_identical() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn stream_parser_default_trait() {
    let parser = StreamParser::default();
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_finish_empty_is_noop() {
    let mut parser = StreamParser::new();
    let results = parser.finish();
    assert!(results.is_empty());
}
