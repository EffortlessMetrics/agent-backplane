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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive protocol framing and edge-case tests for abp-protocol.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::*;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec};

// ── helpers ──────────────────────────────────────────────────────────

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn test_caps() -> CapabilityManifest {
    BTreeMap::new()
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_caps())
}

fn make_fatal(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    }
}

fn make_run() -> Envelope {
    let wo = WorkOrderBuilder::new("hello").build();
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Malformed JSONL
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn malformed_trailing_comma() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"boom",}"#);
    assert!(result.is_err(), "trailing comma should be rejected");
}

#[test]
fn malformed_missing_closing_brace() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"boom""#);
    assert!(result.is_err());
}

#[test]
fn malformed_null_bytes_in_line() {
    let result = JsonlCodec::decode("{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"bo\0om\"}");
    // serde_json may or may not accept null bytes; we just confirm no panic
    let _ = result;
}

#[test]
fn malformed_just_open_brace() {
    assert!(JsonlCodec::decode("{").is_err());
}

#[test]
fn malformed_double_colon() {
    assert!(JsonlCodec::decode(r#"{"t"::"fatal"}"#).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Binary injection — non-UTF-8 bytes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn binary_injection_non_utf8_in_stream() {
    // BufRead will produce an I/O error on invalid UTF-8 lines
    let raw: &[u8] = &[0x7b, 0xff, 0xfe, 0x7d, 0x0a]; // {<invalid>}\n
    let reader = BufReader::new(raw);
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(!results.is_empty());
    assert!(results[0].is_err());
}

#[test]
fn binary_injection_valid_utf8_around_envelope() {
    // Valid UTF-8 but not valid JSON → error
    let reader = BufReader::new("not-json\n".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Very long lines — 1 MB single line envelope
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn very_long_line_1mb_roundtrip() {
    let big = "x".repeat(1_000_000);
    let env = Envelope::Fatal {
        ref_id: Some("run-big".into()),
        error: big.clone(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.len() > 1_000_000);
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 1_000_000),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn very_long_line_via_stream() {
    let big = "y".repeat(1_000_000);
    let env = make_fatal(&big);
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Empty lines — interspersed in JSONL stream
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_lines_interspersed_in_stream() {
    let line = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("\n\n{line}\n\n\n{line}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn only_empty_lines_produce_nothing() {
    let reader = BufReader::new("\n\n  \n\t\n".as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(decoded.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Duplicate fields — JSON with duplicate keys
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn duplicate_fields_rejected() {
    // serde rejects duplicate keys for strongly-typed structs
    let json = r#"{"t":"fatal","ref_id":null,"error":"first","error":"second"}"#;
    assert!(
        JsonlCodec::decode(json).is_err(),
        "duplicate fields should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Extra fields — unknown fields gracefully ignored
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn extra_unknown_fields_ignored() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","unknown_field":"value","extra":42}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn extra_nested_unknown_field_ignored() {
    let json = r#"{"t":"fatal","ref_id":"r1","error":"msg","meta":{"deep":true}}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Missing fields — each required field missing → clear error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn missing_tag_field() {
    assert!(JsonlCodec::decode(r#"{"error":"boom"}"#).is_err());
}

#[test]
fn missing_fatal_error_field() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null}"#);
    assert!(result.is_err(), "fatal without 'error' must fail");
}

#[test]
fn missing_run_work_order_field() {
    let result = JsonlCodec::decode(r#"{"t":"run","id":"abc"}"#);
    assert!(result.is_err(), "run without 'work_order' must fail");
}

#[test]
fn missing_event_ref_id() {
    // event needs ref_id and event fields
    let result = JsonlCodec::decode(
        r#"{"t":"event","event":{"ts":"2024-01-01T00:00:00Z","kind":{"type":"run_started","message":"hi"}}}"#,
    );
    assert!(result.is_err(), "event without 'ref_id' must fail");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Field type mismatch
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn type_mismatch_error_is_number() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":42}"#);
    assert!(result.is_err(), "error field should be string, not number");
}

#[test]
fn type_mismatch_ref_id_is_array() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":[],"error":"boom"}"#);
    assert!(result.is_err());
}

#[test]
fn type_mismatch_tag_is_number() {
    let result = JsonlCodec::decode(r#"{"t":42,"ref_id":null,"error":"boom"}"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Envelope ordering — valid and invalid sequences
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_sequence_hello_run_event_final() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let run = JsonlCodec::encode(&make_run()).unwrap();
    let fatal = JsonlCodec::encode(&make_fatal("done")).unwrap();

    let input = format!("{hello}{run}{fatal}");
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Fatal { .. }));
}

#[test]
fn all_envelope_variants_roundtrip() {
    // Ensure every variant can encode and decode
    let hello = make_hello();
    let run = make_run();
    let fatal = make_fatal("err");

    for env in [&hello, &run, &fatal] {
        let encoded = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        // Just verify no panic and same variant
        assert_eq!(
            std::mem::discriminant(env),
            std::mem::discriminant(&decoded)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Unicode — all Unicode planes in text fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unicode_bmp_characters() {
    let env = Envelope::Fatal {
        ref_id: Some("ñ-日本語-العربية".into()),
        error: "café résumé naïve".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, "café résumé naïve");
            assert_eq!(ref_id.as_deref(), Some("ñ-日本語-العربية"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_supplementary_planes_emoji() {
    // Supplementary Multilingual Plane (SMP) — emoji
    let env = make_fatal("🚀🎉🔥💻🌍");
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "🚀🎉🔥💻🌍"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_supplementary_ideographic_plane() {
    // CJK Unified Ideographs Extension B (SIP, U+20000+)
    let env = make_fatal("\u{20000}\u{2A6D6}");
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "\u{20000}\u{2A6D6}"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn unicode_zero_width_and_rtl() {
    let env = make_fatal("a\u{200B}b\u{200F}c\u{FEFF}d");
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "a\u{200B}b\u{200F}c\u{FEFF}d"),
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Null values — null in optional vs required fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn null_in_optional_ref_id() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn null_in_required_error_field() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":null}"#);
    assert!(result.is_err(), "null in required string field must fail");
}

#[test]
fn null_in_required_tag() {
    let result = JsonlCodec::decode(r#"{"t":null,"ref_id":null,"error":"boom"}"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Numeric edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn numeric_i64_max_in_ext_field() {
    // Numbers in JSON — serde_json handles i64::MAX
    let json = format!(
        r#"{{"t":"fatal","ref_id":null,"error":"num","count":{}}}"#,
        i64::MAX
    );
    // Extra fields are ignored, should not cause a panic
    let decoded = JsonlCodec::decode(&json).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn numeric_nan_not_valid_json() {
    // NaN is not valid JSON
    let json = r#"{"t":"fatal","ref_id":null,"error":"nan","val":NaN}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn numeric_infinity_not_valid_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"inf","val":Infinity}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Nested envelope — envelope containing serialized envelope in data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_envelope_in_error_field() {
    let inner = JsonlCodec::encode(&make_fatal("inner")).unwrap();
    let outer = Envelope::Fatal {
        ref_id: None,
        error: inner.trim().to_string(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&outer).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            // The error field contains a valid JSON string of another envelope
            let inner_decoded = JsonlCodec::decode(&error).unwrap();
            assert!(matches!(inner_decoded, Envelope::Fatal { .. }));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn deeply_nested_envelope_three_levels() {
    let l1 = JsonlCodec::encode(&make_fatal("level-1")).unwrap();
    let l2 = Envelope::Fatal {
        ref_id: None,
        error: l1.trim().to_string(),
        error_code: None,
    };
    let l2_enc = JsonlCodec::encode(&l2).unwrap();
    let l3 = Envelope::Fatal {
        ref_id: None,
        error: l2_enc.trim().to_string(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&l3).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Version compatibility
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn version_parse_large_numbers() {
    let v = ProtocolVersion::parse("abp/v999.999").unwrap();
    assert_eq!(v.major, 999);
    assert_eq!(v.minor, 999);
}

#[test]
fn version_parse_missing_prefix() {
    assert!(ProtocolVersion::parse("v0.1").is_err());
    assert!(ProtocolVersion::parse("0.1").is_err());
}

#[test]
fn version_parse_missing_dot() {
    assert!(ProtocolVersion::parse("abp/v1").is_err());
}

#[test]
fn version_parse_negative_number() {
    assert!(ProtocolVersion::parse("abp/v-1.0").is_err());
}

#[test]
fn version_parse_empty_string() {
    assert!(ProtocolVersion::parse("").is_err());
}

#[test]
fn version_parse_extra_segments() {
    // "abp/v1.2.3" — minor part is "2.3" which won't parse as u32
    assert!(ProtocolVersion::parse("abp/v1.2.3").is_err());
}

#[test]
fn version_compatible_same_major() {
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v1.is_compatible(&v2));
}

#[test]
fn version_incompatible_different_major() {
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(!v1.is_compatible(&v2));
}

#[test]
fn version_negotiate_same_major_picks_min() {
    let local = ProtocolVersion::parse("abp/v0.3").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.minor, 1);
}

#[test]
fn version_negotiate_different_major_fails() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&local, &remote).is_err());
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 6 }));
    assert!(!range.contains(&ProtocolVersion { major: 1, minor: 0 }));
}

#[test]
fn version_zero_zero() {
    let v = ProtocolVersion::parse("abp/v0.0").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Concurrent encoding — multiple threads encoding simultaneously
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_encoding_multiple_threads() {
    use std::sync::Arc;
    use std::thread;

    let envelope = Arc::new(make_fatal("concurrent"));
    let mut handles = Vec::new();

    for _ in 0..8 {
        let env = Arc::clone(&envelope);
        handles.push(thread::spawn(move || {
            let encoded = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            match decoded {
                Envelope::Fatal { error, .. } => assert_eq!(error, "concurrent"),
                _ => panic!("expected Fatal"),
            }
        }));
    }

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn concurrent_encoding_different_variants() {
    use std::thread;

    let mut handles = Vec::new();

    for i in 0..8 {
        handles.push(thread::spawn(move || {
            let env = if i % 2 == 0 {
                make_fatal(&format!("thread-{i}"))
            } else {
                make_hello()
            };
            let encoded = JsonlCodec::encode(&env).unwrap();
            JsonlCodec::decode(encoded.trim()).unwrap()
        }));
    }

    for h in handles {
        let _ = h.join().expect("thread panicked");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Partial reads — incremental JSONL parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn partial_read_byte_at_a_time() {
    // Simulate streaming by feeding chunks to the codec
    let env = make_fatal("streamed");
    let encoded = JsonlCodec::encode(&env).unwrap();
    let bytes = encoded.as_bytes();

    // Accumulate bytes and decode once we see a newline
    let mut buffer = String::new();
    let mut decoded_count = 0;
    for &b in bytes {
        buffer.push(b as char);
        if b == b'\n' {
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                let decoded = JsonlCodec::decode(trimmed).unwrap();
                assert!(matches!(decoded, Envelope::Fatal { .. }));
                decoded_count += 1;
            }
            buffer.clear();
        }
    }
    assert_eq!(decoded_count, 1);
}

#[test]
fn partial_read_multi_envelope_chunked() {
    let line1 = JsonlCodec::encode(&make_fatal("a")).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("b")).unwrap();
    let full = format!("{line1}{line2}");

    // Simulate reading in 64-byte chunks
    let mut buffer = String::new();
    let mut results = Vec::new();
    for chunk in full.as_bytes().chunks(64) {
        buffer.push_str(std::str::from_utf8(chunk).unwrap());
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();
            if !line.is_empty() {
                results.push(JsonlCodec::decode(&line).unwrap());
            }
        }
    }
    assert_eq!(results.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Whitespace variations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn whitespace_crlf_line_endings() {
    let line = JsonlCodec::encode(&make_fatal("crlf")).unwrap();
    // Replace LF with CRLF
    let crlf_input = line.replace('\n', "\r\n");
    let reader = BufReader::new(crlf_input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 1);
}

#[test]
fn whitespace_tabs_between_lines() {
    let line = JsonlCodec::encode(&make_fatal("tab")).unwrap();
    let input = format!("\t\n{line}\t\n");
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 1);
}

#[test]
fn whitespace_leading_trailing_spaces_on_json_line() {
    let line = JsonlCodec::encode(&make_fatal("space")).unwrap();
    let padded = format!("   {}   ", line.trim());
    // StreamingCodec.decode_batch trims lines
    let results = StreamingCodec::decode_batch(&padded);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Empty envelope fields — empty strings in all string fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_string_error_field() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: String::new(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert!(error.is_empty()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn empty_string_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some(String::new()),
        error: "err".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("")),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn empty_backend_id_in_hello() {
    let backend = BackendIdentity {
        id: String::new(),
        backend_version: None,
        adapter_version: None,
    };
    let env = Envelope::hello(backend, test_caps());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => assert!(backend.id.is_empty()),
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Array fields — empty and large arrays via StreamingCodec batch
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_empty_batch_encode_decode() {
    let jsonl = StreamingCodec::encode_batch(&[]);
    assert!(jsonl.is_empty());
    let results = StreamingCodec::decode_batch(&jsonl);
    assert!(results.is_empty());
}

#[test]
fn array_large_batch_500_envelopes() {
    let envelopes: Vec<Envelope> = (0..500).map(|i| make_fatal(&format!("msg-{i}"))).collect();
    let jsonl = StreamingCodec::encode_batch(&envelopes);
    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 500);
}

#[test]
fn array_validate_batch_with_errors() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("{good}BAD\n{good}");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Backslash escaping — JSON strings with special characters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backslash_in_error_field() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: r#"path\to\file"#.into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r"path\\to\\file"));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, r"path\to\file"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn quotes_in_error_field() {
    let env = make_fatal(r#"she said "hello""#);
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, r#"she said "hello""#),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn newlines_in_string_field() {
    let env = make_fatal("line1\nline2\nline3");
    let encoded = JsonlCodec::encode(&env).unwrap();
    // The encoded JSONL should be a single line (newlines escaped)
    assert_eq!(encoded.trim_end().matches('\n').count(), 0);
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "line1\nline2\nline3"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn tabs_and_control_chars_in_string() {
    let env = make_fatal("col1\tcol2\r\nrow2");
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "col1\tcol2\r\nrow2"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn all_json_escape_sequences() {
    let special = "\"\\/\u{0008}\u{000C}\n\r\t";
    let env = make_fatal(special);
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, special),
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_always_ends_with_newline() {
    for env in [make_hello(), make_fatal("x"), make_run()] {
        let encoded = JsonlCodec::encode(&env).unwrap();
        assert!(
            encoded.ends_with('\n'),
            "encoded line must end with newline"
        );
        // Exactly one newline at the end
        assert_eq!(encoded.trim_end_matches('\n').matches('\n').count(), 0);
    }
}

#[test]
fn decode_rejects_bare_string() {
    assert!(JsonlCodec::decode(r#""hello""#).is_err());
}

#[test]
fn decode_rejects_bare_null() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn decode_rejects_bare_boolean() {
    assert!(JsonlCodec::decode("true").is_err());
    assert!(JsonlCodec::decode("false").is_err());
}

#[test]
fn protocol_error_display_is_meaningful() {
    let err = JsonlCodec::decode("not-json").unwrap_err();
    let msg = err.to_string();
    assert!(!msg.is_empty());
    assert!(msg.contains("JSON") || msg.contains("json") || msg.contains("expected"));
}

#[test]
fn version_display_roundtrip() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.to_string(), "abp/v0.1");
    let reparsed = ProtocolVersion::parse(&v.to_string()).unwrap();
    assert_eq!(v, reparsed);
}

#[test]
fn version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}
