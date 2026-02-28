// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::*;
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
    let envelopes = vec![
        make_hello(),
        make_fatal("one"),
        make_fatal("two"),
    ];

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
