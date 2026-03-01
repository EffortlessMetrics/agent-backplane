// SPDX-License-Identifier: MIT OR Apache-2.0
//! Edge-case tests for `abp-protocol`: unusual payloads, line endings, and
//! encoding quirks that real-world JSONL streams may contain.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use std::io::BufReader;

// ── helpers ─────────────────────────────────────────────────────────────

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "edge".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_fatal(msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
    }
}

fn roundtrip_via_stream(raw: &[u8]) -> Vec<Envelope> {
    let reader = BufReader::new(raw);
    JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap()
}

// ── 1. Maximum-size payloads ────────────────────────────────────────────

#[test]
fn large_task_string_in_work_order() {
    let big_task = "A".repeat(256 * 1024); // 256 KB task
    let wo = WorkOrderBuilder::new(&big_task).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();

    match decoded {
        Envelope::Run { work_order, .. } => assert_eq!(work_order.task.len(), 256 * 1024),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn large_error_via_writer_stream() {
    let big_error = "E".repeat(512 * 1024);
    let env = Envelope::Fatal {
        ref_id: Some("big-run".into()),
        error: big_error.clone(),
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();

    let decoded = roundtrip_via_stream(&buf);
    assert_eq!(decoded.len(), 1);
    match &decoded[0] {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 512 * 1024),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── 2. Special characters in strings ────────────────────────────────────

#[test]
fn embedded_newlines_in_error_message() {
    let msg = "line1\nline2\nline3";
    let env = make_fatal(msg);

    let encoded = JsonlCodec::encode(&env).unwrap();
    // JSON-encodes newlines as \n escape inside the string, so the
    // JSONL line itself must still be a single line.
    assert_eq!(encoded.matches('\n').count(), 1);

    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn null_bytes_in_string() {
    let msg = "before\0after";
    let env = make_fatal(msg);

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn unicode_characters_in_strings() {
    let msg = "日本語テスト 🦀 émojis ñ àçcénts «»";
    let env = make_fatal(msg);

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn tab_and_carriage_return_in_string() {
    let msg = "col1\tcol2\r\nrow2";
    let env = make_fatal(msg);

    let encoded = JsonlCodec::encode(&env).unwrap();
    assert_eq!(
        encoded.matches('\n').count(),
        1,
        "must be single JSONL line"
    );
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, msg),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── 3. CRLF line endings ────────────────────────────────────────────────

#[test]
fn crlf_line_endings() {
    let line1 = JsonlCodec::encode(&make_hello()).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("crlf")).unwrap();

    // Replace LF with CRLF to simulate a Windows-originated stream.
    let stream = format!("{}\r\n{}\r\n", line1.trim_end(), line2.trim_end());
    let decoded = roundtrip_via_stream(stream.as_bytes());

    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    match &decoded[1] {
        Envelope::Fatal { error, .. } => assert_eq!(error, "crlf"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── 4. Mixed line endings ───────────────────────────────────────────────

#[test]
fn mixed_line_endings() {
    let l1 = JsonlCodec::encode(&make_hello()).unwrap();
    let l2 = JsonlCodec::encode(&make_fatal("a")).unwrap();
    let l3 = JsonlCodec::encode(&make_fatal("b")).unwrap();

    // LF, CRLF, LF — mixed in a single stream.
    let stream = format!("{}\n{}\r\n{}", l1.trim_end(), l2.trim_end(), l3.trim_end());
    let decoded = roundtrip_via_stream(stream.as_bytes());

    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(&decoded[1], Envelope::Fatal { error, .. } if error == "a"));
    assert!(matches!(&decoded[2], Envelope::Fatal { error, .. } if error == "b"));
}

// ── 5. Very long JSONL lines (100KB+) ───────────────────────────────────

#[test]
fn very_long_jsonl_line() {
    let big = "x".repeat(100 * 1024);
    let env = Envelope::Fatal {
        ref_id: Some("long-line".into()),
        error: big.clone(),
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    // The serialized line is > 100 KB.
    assert!(buf.len() > 100 * 1024);

    let decoded = roundtrip_via_stream(&buf);
    assert_eq!(decoded.len(), 1);
    match &decoded[0] {
        Envelope::Fatal { error, ref_id } => {
            assert_eq!(error, &big);
            assert_eq!(ref_id.as_deref(), Some("long-line"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ── 6. Empty ref_id fields ─────────────────────────────────────────────

#[test]
fn empty_ref_id_on_event() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: String::new(),
        event,
    };

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert!(ref_id.is_empty()),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn empty_ref_id_on_final() {
    let receipt = Receipt {
        meta: RunMetadata {
            run_id: uuid::Uuid::new_v4(),
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: std::collections::BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let env = Envelope::Final {
        ref_id: String::new(),
        receipt,
    };

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Final { ref_id, .. } => assert!(ref_id.is_empty()),
        other => panic!("expected Final, got {other:?}"),
    }
}

// ── 7. UTF-8 BOM at start of stream ────────────────────────────────────

#[test]
fn utf8_bom_at_start_of_stream() {
    let line = JsonlCodec::encode(&make_fatal("bom-test")).unwrap();
    // Prepend UTF-8 BOM (EF BB BF).
    let mut stream = Vec::from(b"\xEF\xBB\xBF" as &[u8]);
    stream.extend_from_slice(line.as_bytes());

    let reader = BufReader::new(stream.as_slice());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    // BOM before the first `{` will cause a parse error for the first line
    // because serde expects `{` as the first character. This is expected
    // behaviour — we document that the codec does not strip BOMs.
    assert_eq!(results.len(), 1);
    assert!(
        results[0].is_err(),
        "UTF-8 BOM should cause a parse error on the first line"
    );
}

#[test]
fn utf8_bom_does_not_affect_subsequent_lines() {
    let line1 = JsonlCodec::encode(&make_hello()).unwrap();
    let line2 = JsonlCodec::encode(&make_fatal("after-bom")).unwrap();

    let mut stream = Vec::from(b"\xEF\xBB\xBF" as &[u8]);
    stream.extend_from_slice(line1.as_bytes());
    stream.extend_from_slice(line2.as_bytes());

    let reader = BufReader::new(stream.as_slice());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    assert_eq!(results.len(), 2);
    // First line fails due to BOM prefix.
    assert!(results[0].is_err());
    // Second line parses fine.
    assert!(results[1].is_ok());
    match results[1].as_ref().unwrap() {
        Envelope::Fatal { error, .. } => assert_eq!(error, "after-bom"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}
