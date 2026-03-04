// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep, comprehensive tests for the JSONL protocol codec.
//!
//! Categories:
//! 1. Encode – Envelope → JSONL line
//! 2. Decode – JSONL line → Envelope
//! 3. Roundtrip – Encode → Decode identity
//! 4. Multi-line – Multiple envelopes in one stream
//! 5. Partial reads – Incomplete line handling (StreamParser)
//! 6. Invalid JSON – Malformed JSON error handling
//! 7. Unknown tag – Unknown envelope type handling
//! 8. Large payloads – Large envelope handling
//! 9. Empty lines – Blank line handling
//! 10. UTF-8 – Unicode content in payloads
//! 11. Concurrent codec – Thread-safe encoding/decoding
//! 12. Streaming codec – StreamParser framing

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::*;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

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

fn make_fatal(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    }
}

fn make_fatal_with_ref(msg: &str, ref_id: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: Some(ref_id.into()),
        error: msg.into(),
        error_code: None,
    }
}

fn make_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
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

fn make_final_envelope(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Encode – Envelope → JSONL line
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_hello_ends_with_newline() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn encode_hello_contains_tag() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(line.contains(r#""t":"hello""#));
}

#[test]
fn encode_hello_contains_contract_version() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(line.contains(CONTRACT_VERSION));
}

#[test]
fn encode_fatal_contains_tag() {
    let line = JsonlCodec::encode(&make_fatal("boom")).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn encode_fatal_contains_error_message() {
    let line = JsonlCodec::encode(&make_fatal("something broke")).unwrap();
    assert!(line.contains("something broke"));
}

#[test]
fn encode_event_contains_tag() {
    let line = JsonlCodec::encode(&make_event("run-1", "hello")).unwrap();
    assert!(line.contains(r#""t":"event""#));
}

#[test]
fn encode_event_contains_ref_id() {
    let line = JsonlCodec::encode(&make_event("run-42", "msg")).unwrap();
    assert!(line.contains(r#""ref_id":"run-42""#));
}

#[test]
fn encode_run_contains_tag() {
    let line = JsonlCodec::encode(&make_run()).unwrap();
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn encode_final_contains_tag() {
    let line = JsonlCodec::encode(&make_final_envelope("run-1")).unwrap();
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn encode_produces_single_line() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    assert_eq!(line.matches('\n').count(), 1, "must be exactly one newline");
    assert!(line.ends_with('\n'));
}

#[test]
fn encode_produces_valid_json() {
    let line = JsonlCodec::encode(&make_hello()).unwrap();
    let _: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
}

#[test]
fn encode_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "rate limited",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("rate limited"));
    assert!(line.contains("error_code"));
}

#[test]
fn encode_to_writer_produces_valid_jsonl() {
    let env = make_fatal("writer-test");
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    assert_eq!(output.matches('\n').count(), 1);
}

#[test]
fn encode_many_to_writer_multiple_lines() {
    let envs = vec![make_hello(), make_fatal("a"), make_fatal("b")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches('\n').count(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Decode – JSONL line → Envelope
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_fatal_from_raw_json() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
}

#[test]
fn decode_hello_from_raw_json() {
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"s1","backend_version":null,"adapter_version":null}},"capabilities":{{}},"mode":"mapped"}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn decode_fatal_with_ref_id() {
    let line = r#"{"t":"fatal","ref_id":"run-99","error":"oops"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-99"));
            assert_eq!(error, "oops");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_fatal_null_ref_id() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"no-ref"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn decode_strips_trailing_newline() {
    let encoded = JsonlCodec::encode(&make_fatal("test")).unwrap();
    let env = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "test"));
}

#[test]
fn decode_fatal_with_error_code_field() {
    let line =
        r#"{"t":"fatal","ref_id":null,"error":"bad","error_code":"protocol_invalid_envelope"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(
        env,
        Envelope::Fatal {
            error_code: Some(_),
            ..
        }
    ));
}

#[test]
fn decode_hello_default_mode() {
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"test"}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&json).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Roundtrip – Encode → Decode identity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_hello() {
    let original = make_hello();
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
    if let (Envelope::Hello { backend: b1, .. }, Envelope::Hello { backend: b2, .. }) =
        (&original, &decoded)
    {
        assert_eq!(b1.id, b2.id);
    }
}

#[test]
fn roundtrip_fatal() {
    let original = make_fatal("roundtrip-error");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(&decoded, Envelope::Fatal { error, .. } if error == "roundtrip-error"));
}

#[test]
fn roundtrip_fatal_with_ref() {
    let original = make_fatal_with_ref("err", "run-7");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-7"));
            assert_eq!(error, "err");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn roundtrip_event() {
    let original = make_event("run-1", "hello world");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "hello world"),
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn roundtrip_run() {
    let original = make_run();
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn roundtrip_final() {
    let original = make_final_envelope("run-1");
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn roundtrip_preserves_json_equality() {
    let original = make_fatal("json-eq");
    let line1 = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line1.trim()).unwrap();
    let line2 = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(line1, line2);
}

#[test]
fn roundtrip_hello_passthrough_mode() {
    let original = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Multi-line – Multiple envelopes in one stream
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multiline_decode_stream_two_envelopes() {
    let line1 = JsonlCodec::encode(&make_hello()).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("done")).unwrap();
    let input = format!("{line1}{line2}");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(&envelopes[1], Envelope::Fatal { error, .. } if error == "done"));
}

#[test]
fn multiline_decode_stream_five_envelopes() {
    let envs: Vec<_> = (0..5).map(|i| make_fatal(&format!("msg-{i}"))).collect();
    let mut input = String::new();
    for e in &envs {
        input.push_str(&JsonlCodec::encode(e).unwrap());
    }

    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 5);
}

#[test]
fn multiline_full_protocol_sequence() {
    let hello = make_hello();
    let run = make_run();
    let run_id = match &run {
        Envelope::Run { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    let event = make_event(&run_id, "working...");
    let fin = make_final_envelope(&run_id);

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, run, event, fin]).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

#[test]
fn multiline_writer_reader_roundtrip() {
    let envs = vec![make_hello(), make_fatal("one"), make_fatal("two")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn multiline_streaming_codec_batch() {
    let envs: Vec<_> = (0..10).map(|i| make_fatal(&format!("batch-{i}"))).collect();
    let jsonl = StreamingCodec::encode_batch(&envs);
    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Partial reads – StreamParser incomplete line handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn partial_read_no_complete_line() {
    let mut parser = StreamParser::new();
    let results = parser.push(b"{\"t\":\"fatal\"");
    assert!(results.is_empty());
    assert!(!parser.is_empty());
}

#[test]
fn partial_read_completes_on_second_push() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("partial")).unwrap();
    let bytes = line.as_bytes();
    let mid = bytes.len() / 2;

    let r1 = parser.push(&bytes[..mid]);
    assert!(r1.is_empty());

    let r2 = parser.push(&bytes[mid..]);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn partial_read_byte_by_byte() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("byte-by-byte")).unwrap();
    let bytes = line.as_bytes();

    let mut total_results = Vec::new();
    for &b in bytes {
        total_results.extend(parser.push(&[b]));
    }
    assert_eq!(total_results.len(), 1);
    assert!(total_results[0].is_ok());
}

#[test]
fn partial_read_multiple_lines_in_one_chunk() {
    let mut parser = StreamParser::new();
    let line1 = JsonlCodec::encode(&make_fatal("first")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("second")).unwrap();
    let combined = format!("{line1}{line2}");

    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
}

#[test]
fn partial_read_finish_flushes_unterminated_line() {
    let mut parser = StreamParser::new();
    // Push a complete JSON without trailing newline
    let json = r#"{"t":"fatal","ref_id":null,"error":"no-newline"}"#;
    let r1 = parser.push(json.as_bytes());
    assert!(r1.is_empty());

    let r2 = parser.finish();
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn partial_read_buffered_len_tracks_state() {
    let mut parser = StreamParser::new();
    assert_eq!(parser.buffered_len(), 0);

    parser.push(b"partial data");
    assert!(parser.buffered_len() > 0);

    parser.reset();
    assert_eq!(parser.buffered_len(), 0);
    assert!(parser.is_empty());
}

#[test]
fn partial_read_three_fragments() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("three-parts")).unwrap();
    let bytes = line.as_bytes();
    let third = bytes.len() / 3;

    let r1 = parser.push(&bytes[..third]);
    assert!(r1.is_empty());

    let r2 = parser.push(&bytes[third..third * 2]);
    assert!(r2.is_empty());

    let r3 = parser.push(&bytes[third * 2..]);
    assert_eq!(r3.len(), 1);
    assert!(r3[0].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Invalid JSON – Malformed JSON error handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn invalid_json_plain_text() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_empty_object() {
    let err = JsonlCodec::decode("{}").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_array() {
    let err = JsonlCodec::decode("[]").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_number() {
    let err = JsonlCodec::decode("42").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_null() {
    let err = JsonlCodec::decode("null").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_truncated_object() {
    let err = JsonlCodec::decode(r#"{"t":"fatal","error":"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_missing_closing_brace() {
    let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"boom""#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_double_comma() {
    let err = JsonlCodec::decode(r#"{"t":"fatal",,"error":"x"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn invalid_json_stream_parser_reports_error() {
    let mut parser = StreamParser::new();
    let results = parser.push(b"INVALID JSON\n");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn invalid_json_streaming_codec_reports_errors() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("{good}NOT JSON\n{good}");
    let results = StreamingCodec::decode_batch(&input);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Unknown tag – Unknown envelope type handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_tag_string() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type","data":123}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_typo_in_hello() {
    let err = JsonlCodec::decode(r#"{"t":"helo","backend":{},"capabilities":{}}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_uppercase() {
    let err = JsonlCodec::decode(r#"{"t":"FATAL","ref_id":null,"error":"boom"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_camel_case() {
    let err = JsonlCodec::decode(r#"{"t":"Hello","backend":{}}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_empty_string() {
    let err = JsonlCodec::decode(r#"{"t":"","data":{}}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_numeric() {
    let err = JsonlCodec::decode(r#"{"t":42,"data":{}}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_tag_missing_t_field() {
    let err = JsonlCodec::decode(r#"{"type":"fatal","error":"boom"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Large payloads – Large envelope handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_payload_1mb_roundtrip() {
    let big_text = "A".repeat(1_000_000);
    let env = Envelope::Fatal {
        ref_id: Some("run-large".into()),
        error: big_text.clone(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, big_text);
            assert_eq!(ref_id.as_deref(), Some("run-large"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn large_payload_via_writer_reader() {
    let big_text = "Z".repeat(500_000);
    let env = make_fatal(&big_text);
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let mut iter = JsonlCodec::decode_stream(reader);
    let decoded = iter.next().unwrap().unwrap();
    assert!(iter.next().is_none());

    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error.len(), 500_000);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn large_payload_stream_parser() {
    let big_text = "B".repeat(100_000);
    let env = make_fatal(&big_text);
    let line = JsonlCodec::encode(&env).unwrap();

    let mut parser = StreamParser::new();
    // Feed in 1KB chunks
    let bytes = line.as_bytes();
    let mut results = Vec::new();
    for chunk in bytes.chunks(1024) {
        results.extend(parser.push(chunk));
    }
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn large_payload_streaming_codec_batch() {
    let envs: Vec<_> = (0..100)
        .map(|i| {
            let text = format!("{}-{}", "x".repeat(10_000), i);
            make_fatal(&text)
        })
        .collect();
    let jsonl = StreamingCodec::encode_batch(&envs);
    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 100);
}

#[test]
fn large_payload_stream_parser_max_line_len() {
    let mut parser = StreamParser::with_max_line_len(100);
    let big = format!(
        "{}{}\n",
        r#"{"t":"fatal","ref_id":null,"error":""#,
        "x".repeat(200)
    );
    let results = parser.push(big.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Empty lines – Blank line handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_lines_decode_stream_empty_input() {
    let reader = BufReader::new(b"" as &[u8]);
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(envelopes.is_empty());
}

#[test]
fn empty_lines_decode_stream_only_blanks() {
    let reader = BufReader::new(b"\n\n  \n\t\n" as &[u8]);
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn empty_lines_between_valid_envelopes() {
    let line = JsonlCodec::encode(&make_fatal("spaced")).unwrap();
    let input = format!("\n\n{line}\n  \n{line}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn empty_lines_stream_parser_skips_blanks() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("blanks")).unwrap();
    let input = format!("\n\n{line}\n\n");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn empty_lines_streaming_codec_skips_blanks() {
    let line = JsonlCodec::encode(&make_fatal("x")).unwrap();
    let input = format!("\n{line}\n\n{line}\n  \n");
    let decoded = StreamingCodec::decode_batch(&input);
    assert_eq!(decoded.len(), 2);
}

#[test]
fn empty_lines_whitespace_only_lines() {
    let line = JsonlCodec::encode(&make_fatal("ws")).unwrap();
    let input = format!("   \n\t\t\n{line}  \r\n", line = line.trim());
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. UTF-8 – Unicode content in payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn utf8_japanese_error_message() {
    let env = make_fatal("エラーが発生しました");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "エラーが発生しました"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn utf8_emoji_in_payload() {
    let env = make_fatal("🚀💻🔥 error occurred");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "🚀💻🔥 error occurred"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn utf8_chinese_arabic_mixed() {
    let env = Envelope::Fatal {
        ref_id: Some("运行-1".into()),
        error: "中文 العربية".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("运行-1"));
            assert_eq!(error, "中文 العربية");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn utf8_accented_characters() {
    let env = make_fatal("résumé café naïve Ñoño");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "résumé café naïve Ñoño"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn utf8_zero_width_and_special_chars() {
    // Zero-width joiner, combining chars, RTL mark
    let special = "a\u{200D}b\u{0301}\u{200F}c";
    let env = make_fatal(special);
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, special),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn utf8_event_with_unicode() {
    let env = make_event("run-日本", "こんにちは世界 🌍");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-日本");
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, "こんにちは世界 🌍")
                }
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn utf8_stream_parser_handles_multibyte() {
    let mut parser = StreamParser::new();
    let env = make_fatal("日本語テスト");
    let line = JsonlCodec::encode(&env).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn utf8_invalid_bytes_stream_parser() {
    let mut parser = StreamParser::new();
    // Invalid UTF-8 sequence
    let invalid: &[u8] = &[0xFF, 0xFE, b'\n'];
    let results = parser.push(invalid);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Concurrent codec – Thread-safe encoding/decoding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_encode_from_multiple_threads() {
    use std::thread;

    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let env = make_fatal(&format!("thread-{i}"));
                JsonlCodec::encode(&env).unwrap()
            })
        })
        .collect();

    let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.len(), 10);
    for line in &results {
        assert!(line.ends_with('\n'));
        assert!(line.contains("\"t\":\"fatal\""));
    }
}

#[test]
fn concurrent_decode_from_multiple_threads() {
    use std::thread;

    let lines: Vec<String> = (0..10)
        .map(|i| JsonlCodec::encode(&make_fatal(&format!("msg-{i}"))).unwrap())
        .collect();

    let handles: Vec<_> = lines
        .into_iter()
        .map(|line| {
            thread::spawn(move || {
                let trimmed = line.trim().to_string();
                JsonlCodec::decode(&trimmed).unwrap()
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.len(), 10);
    for env in &results {
        assert!(matches!(env, Envelope::Fatal { .. }));
    }
}

#[test]
fn concurrent_roundtrip_from_multiple_threads() {
    use std::thread;

    let handles: Vec<_> = (0..20)
        .map(|i| {
            thread::spawn(move || {
                let env = make_fatal(&format!("concurrent-{i}"));
                let line = JsonlCodec::encode(&env).unwrap();
                let decoded = JsonlCodec::decode(line.trim()).unwrap();
                match decoded {
                    Envelope::Fatal { error, .. } => {
                        assert_eq!(error, format!("concurrent-{i}"));
                    }
                    _ => panic!("expected Fatal"),
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_stream_parser_per_thread() {
    use std::thread;

    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let mut parser = StreamParser::new();
                let env = make_fatal(&format!("parser-{i}"));
                let line = JsonlCodec::encode(&env).unwrap();
                let results = parser.push(line.as_bytes());
                assert_eq!(results.len(), 1);
                assert!(results[0].is_ok());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Streaming codec – StreamParser framing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_parser_new_is_empty() {
    let parser = StreamParser::new();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn streaming_parser_default_is_empty() {
    let parser = StreamParser::default();
    assert!(parser.is_empty());
}

#[test]
fn streaming_parser_feed_alias() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("feed")).unwrap();
    let results = parser.feed(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn streaming_parser_interleaved_partial_and_complete() {
    let mut parser = StreamParser::new();
    let line1 = JsonlCodec::encode(&make_fatal("first")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("second")).unwrap();

    // Push half of line1
    let half = line1.len() / 2;
    let r1 = parser.push(&line1.as_bytes()[..half]);
    assert!(r1.is_empty());

    // Push rest of line1 + all of line2
    let combined = format!("{}{line2}", &line1[half..]);
    let r2 = parser.push(combined.as_bytes());
    assert_eq!(r2.len(), 2);
}

#[test]
fn streaming_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data without newline");
    assert!(!parser.is_empty());

    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn streaming_parser_finish_on_empty_is_noop() {
    let mut parser = StreamParser::new();
    let results = parser.finish();
    assert!(results.is_empty());
}

#[test]
fn streaming_parser_finish_with_valid_unterminated() {
    let mut parser = StreamParser::new();
    let json = r#"{"t":"fatal","ref_id":null,"error":"unterminated"}"#;
    parser.push(json.as_bytes());
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn streaming_parser_finish_with_invalid_unterminated() {
    let mut parser = StreamParser::new();
    parser.push(b"not json at all");
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn streaming_parser_skips_blank_lines_in_feed() {
    let mut parser = StreamParser::new();
    let line = JsonlCodec::encode(&make_fatal("after-blanks")).unwrap();
    let input = format!("\n\n  \n{line}");
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn streaming_parser_large_multi_chunk_stream() {
    let mut parser = StreamParser::new();
    let envs: Vec<_> = (0..50).map(|i| make_fatal(&format!("item-{i}"))).collect();
    let mut all_bytes = Vec::new();
    for env in &envs {
        all_bytes.extend_from_slice(JsonlCodec::encode(env).unwrap().as_bytes());
    }

    // Feed in 256-byte chunks
    let mut total_results = Vec::new();
    for chunk in all_bytes.chunks(256) {
        total_results.extend(parser.push(chunk));
    }
    total_results.extend(parser.finish());
    assert_eq!(total_results.len(), 50);
    for r in &total_results {
        assert!(r.is_ok());
    }
}

#[test]
fn streaming_parser_custom_max_line_len() {
    let parser = StreamParser::with_max_line_len(512);
    assert!(parser.is_empty());
}

#[test]
fn streaming_codec_line_count_accuracy() {
    let line = JsonlCodec::encode(&make_fatal("count")).unwrap();
    let input = format!("{line}{line}\n  \n{line}");
    assert_eq!(StreamingCodec::line_count(&input), 3);
}

#[test]
fn streaming_codec_validate_finds_bad_lines() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("{good}BAD LINE\n{good}{{invalid}}\n");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_display_json() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_error_code_for_violation() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_error_code_for_unexpected() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "fatal".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn envelope_error_code_on_fatal_with_code() {
    let env =
        Envelope::fatal_with_code(None, "test", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn envelope_error_code_on_non_fatal() {
    let env = make_hello();
    assert!(env.error_code().is_none());
}

#[test]
fn decode_extra_fields_ignored() {
    // Extra fields beyond the known schema should be ignored gracefully
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
}

#[test]
fn encode_deterministic_for_same_input() {
    let env = make_fatal("deterministic");
    let line1 = JsonlCodec::encode(&env).unwrap();
    let line2 = JsonlCodec::encode(&env).unwrap();
    assert_eq!(line1, line2);
}
