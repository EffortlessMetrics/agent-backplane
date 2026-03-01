// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::*;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::{Envelope, JsonlCodec};
use std::collections::BTreeMap;
use std::io::BufReader;

// ── Helpers ──────────────────────────────────────────────────────────────

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

fn make_fatal(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

// ── decode_stream from multi-line JSONL ─────────────────────────────────

#[test]
fn decode_stream_multi_line() {
    let line1 = JsonlCodec::encode(&make_hello()).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("boom")).unwrap();
    let input = format!("{line1}{line2}");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(&envelopes[1], Envelope::Fatal { error, .. } if error == "boom"));
}

// ── decode_stream handles empty input ───────────────────────────────────

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new(b"" as &[u8]);
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(envelopes.is_empty());
}

// ── decode_stream handles blank lines ───────────────────────────────────

#[test]
fn decode_stream_skips_blank_lines() {
    let line = JsonlCodec::encode(&make_fatal("err")).unwrap();
    let input = format!("\n  \n{line}\n\n");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 1);
    assert!(matches!(&envelopes[0], Envelope::Fatal { error, .. } if error == "err"));
}

// ── encode_to_writer produces valid JSONL ───────────────────────────────

#[test]
fn encode_to_writer_valid_jsonl() {
    let env = make_fatal("test");
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();

    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    assert_eq!(output.matches('\n').count(), 1);
    // Must be parseable JSON
    let _: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
}

// ── encode → decode round-trip via writer/reader ────────────────────────

#[test]
fn roundtrip_writer_reader() {
    let envelopes = vec![make_hello(), make_fatal("one"), make_fatal("two")];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(&decoded[1], Envelope::Fatal { error, .. } if error == "one"));
    assert!(matches!(&decoded[2], Envelope::Fatal { error, .. } if error == "two"));
}

// ── Large payloads work correctly ───────────────────────────────────────

#[test]
fn large_payload_roundtrip() {
    let big_text = "x".repeat(1_000_000);
    let env = Envelope::Fatal {
        ref_id: Some("run-large".into()),
        error: big_text.clone(),
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let mut iter = JsonlCodec::decode_stream(reader);
    let decoded = iter.next().unwrap().unwrap();
    assert!(iter.next().is_none());

    if let Envelope::Fatal { error, ref_id } = decoded {
        assert_eq!(error, big_text);
        assert_eq!(ref_id.as_deref(), Some("run-large"));
    } else {
        panic!("expected Fatal variant");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// StreamingCodec tests
// ═══════════════════════════════════════════════════════════════════════

// ── encode_batch → decode_batch round-trip ──────────────────────────────

#[test]
fn streaming_encode_decode_roundtrip() {
    let envelopes = vec![make_hello(), make_fatal("one"), make_fatal("two")];
    let jsonl = StreamingCodec::encode_batch(&envelopes);
    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(&decoded[1], Envelope::Fatal { error, .. } if error == "one"));
    assert!(matches!(&decoded[2], Envelope::Fatal { error, .. } if error == "two"));
}

// ── mixed valid/invalid lines ──────────────────────────────────────────

#[test]
fn streaming_decode_mixed_valid_invalid() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("{good}not-json\n{good}");
    let results = StreamingCodec::decode_batch(&input);

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ── empty input ────────────────────────────────────────────────────────

#[test]
fn streaming_decode_empty_input() {
    let results = StreamingCodec::decode_batch("");
    assert!(results.is_empty());
}

// ── encode_batch empty ─────────────────────────────────────────────────

#[test]
fn streaming_encode_batch_empty() {
    let jsonl = StreamingCodec::encode_batch(&[]);
    assert!(jsonl.is_empty());
}

// ── single line ────────────────────────────────────────────────────────

#[test]
fn streaming_single_line() {
    let envelopes = vec![make_fatal("only")];
    let jsonl = StreamingCodec::encode_batch(&envelopes);
    let decoded = StreamingCodec::decode_batch(&jsonl);

    assert_eq!(decoded.len(), 1);
    let env = decoded.into_iter().next().unwrap().unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "only"));
}

// ── trailing newline handling ──────────────────────────────────────────

#[test]
fn streaming_trailing_newline() {
    let line = JsonlCodec::encode(&make_fatal("x")).unwrap();
    // With trailing newline — should not produce an extra entry
    let with_trailing = format!("{line}\n");
    let without_trailing = line.trim_end().to_string();

    let r1 = StreamingCodec::decode_batch(&with_trailing);
    let r2 = StreamingCodec::decode_batch(&without_trailing);

    assert_eq!(r1.len(), 1);
    assert_eq!(r2.len(), 1);
}

// ── line_count accuracy ────────────────────────────────────────────────

#[test]
fn streaming_line_count() {
    let line = JsonlCodec::encode(&make_fatal("a")).unwrap();
    let input = format!("{line}{line}\n  \n{line}");
    assert_eq!(StreamingCodec::line_count(&input), 3);
}

#[test]
fn streaming_line_count_empty() {
    assert_eq!(StreamingCodec::line_count(""), 0);
    assert_eq!(StreamingCodec::line_count("\n\n  \n"), 0);
}

// ── validate_jsonl with errors at specific lines ───────────────────────

#[test]
fn streaming_validate_jsonl_errors() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    // Line 1: good, line 2: bad, line 3: good, line 4: bad
    let input = format!("{good}INVALID\n{good}{{broken}}");
    let errors = StreamingCodec::validate_jsonl(&input);

    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].0, 2); // 1-based line number
    assert_eq!(errors[1].0, 4);
}

#[test]
fn streaming_validate_jsonl_all_valid() {
    let good = JsonlCodec::encode(&make_fatal("ok")).unwrap();
    let input = format!("{good}{good}");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert!(errors.is_empty());
}

// ── large batch (1000 envelopes) ───────────────────────────────────────

#[test]
fn streaming_large_batch() {
    let envelopes: Vec<Envelope> = (0..1000).map(|i| make_fatal(&format!("msg-{i}"))).collect();

    let jsonl = StreamingCodec::encode_batch(&envelopes);
    assert_eq!(StreamingCodec::line_count(&jsonl), 1000);

    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 1000);

    // Spot-check first and last
    assert!(matches!(&decoded[0], Envelope::Fatal { error, .. } if error == "msg-0"));
    assert!(matches!(&decoded[999], Envelope::Fatal { error, .. } if error == "msg-999"));
}

// ── whitespace handling ────────────────────────────────────────────────

#[test]
fn streaming_whitespace_handling() {
    let line = JsonlCodec::encode(&make_fatal("ws")).unwrap();
    // Leading/trailing spaces around valid JSON, blank lines between
    let input = format!("  {}\n\n  \n  {}", line.trim(), line.trim());
    let decoded: Vec<_> = StreamingCodec::decode_batch(&input)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

// ── UTF-8 content in envelopes ─────────────────────────────────────────

#[test]
fn streaming_utf8_content() {
    let envelopes = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "日本語テスト 🚀 émojis & ñ".into(),
        },
        Envelope::Fatal {
            ref_id: Some("ü-ref".into()),
            error: "中文 العربية".into(),
        },
    ];

    let jsonl = StreamingCodec::encode_batch(&envelopes);
    let decoded: Vec<_> = StreamingCodec::decode_batch(&jsonl)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 2);
    assert!(
        matches!(&decoded[0], Envelope::Fatal { error, .. } if error == "日本語テスト 🚀 émojis & ñ")
    );
    assert!(
        matches!(&decoded[1], Envelope::Fatal { error, ref_id } if error == "中文 العربية" && ref_id.as_deref() == Some("ü-ref"))
    );
}
