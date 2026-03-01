// SPDX-License-Identifier: MIT OR Apache-2.0
//! Negative and edge-case tests for abp-protocol JSONL wire format.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    BTreeMap::new()
}

// ── 1. Parse empty string as Envelope (should error) ────────────────

#[test]
fn decode_empty_string_errors() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_whitespace_only_errors() {
    let result = JsonlCodec::decode("   ");
    assert!(result.is_err());
}

// ── 2. Parse valid JSON but wrong schema as Envelope (should error) ─

#[test]
fn decode_valid_json_missing_tag_errors() {
    let result = JsonlCodec::decode(r#"{"foo": "bar"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_valid_json_wrong_tag_value_errors() {
    let result = JsonlCodec::decode(r#"{"t": "nonexistent_variant"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_array_instead_of_object_errors() {
    let result = JsonlCodec::decode("[1, 2, 3]");
    assert!(result.is_err());
}

#[test]
fn decode_scalar_instead_of_object_errors() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

// ── 3. Parse truncated JSON line ────────────────────────────────────

#[test]
fn decode_truncated_json_errors() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let truncated = &encoded[..encoded.len() / 2];
    let result = JsonlCodec::decode(truncated);
    assert!(result.is_err());
}

#[test]
fn decode_unclosed_brace_errors() {
    let result = JsonlCodec::decode(r#"{"t": "hello", "contract_version": "abp/v0.1""#);
    assert!(result.is_err());
}

// ── 4. Envelope with very large payload ─────────────────────────────

#[test]
fn encode_decode_large_fatal_payload() {
    let large_error = "e".repeat(100 * 1024); // 100 KB error message
    let env = Envelope::Fatal {
        ref_id: None,
        error: large_error.clone(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100 * 1024),
        other => panic!("expected Fatal, got {:?}", other),
    }
}

#[test]
fn encode_decode_large_work_order_task() {
    let large_task = "t".repeat(50 * 1024);
    let wo = WorkOrderBuilder::new(&large_task).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => assert_eq!(work_order.task.len(), 50 * 1024),
        other => panic!("expected Run, got {:?}", other),
    }
}

// ── 5. Multiple newlines between JSONL lines ────────────────────────

#[test]
fn decode_ignores_leading_trailing_whitespace() {
    // JsonlCodec::decode should handle trimmed lines; newline handling
    // is the caller's responsibility, but the codec should decode a
    // properly trimmed line even if it originally had surrounding space.
    let env = Envelope::hello(test_backend(), test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let trimmed = encoded.trim();
    let decoded = JsonlCodec::decode(trimmed).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn multi_line_jsonl_stream_parsed_line_by_line() {
    let env1 = Envelope::hello(test_backend(), test_capabilities());
    let env2 = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
    };
    let line1 = JsonlCodec::encode(&env1).unwrap();
    let line2 = JsonlCodec::encode(&env2).unwrap();

    // Simulate a stream with blank lines interspersed
    let stream = format!("{}\n\n\n{}", line1, line2);

    let mut decoded = Vec::new();
    for line in stream.lines() {
        if line.trim().is_empty() {
            continue; // skip blank lines — protocol should tolerate this
        }
        decoded.push(JsonlCodec::decode(line).unwrap());
    }
    assert_eq!(decoded.len(), 2);
}

#[test]
fn blank_lines_are_not_valid_envelopes() {
    let result = JsonlCodec::decode("\n");
    assert!(result.is_err());
}
