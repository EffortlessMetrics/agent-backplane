// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for `abp-sidecar-utils` protocol helpers.

use abp_core::{AgentEvent, AgentEventKind, CONTRACT_VERSION, Outcome, ReceiptBuilder};
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_utils::frame::{
    decode_envelope, encode_envelope, encode_event, encode_fatal, encode_final, encode_hello,
};
use abp_sidecar_utils::testing::{mock_event, mock_fatal, mock_final, mock_hello, mock_work_order};
use abp_sidecar_utils::validate::{validate_hello, validate_ref_id, validate_sequence};
use chrono::Utc;

// ========================================================================
// Frame helpers — encode/decode roundtrip
// ========================================================================

#[test]
fn roundtrip_hello() {
    let line = encode_hello("test-sidecar", "1.0", &["streaming", "tool_read"]);
    let env = decode_envelope(&line).unwrap();
    match &env {
        Envelope::Hello {
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "test-sidecar");
            assert!(!capabilities.is_empty());
        }
        _ => panic!("expected Hello envelope"),
    }
    // Re-encode and re-decode should also work.
    let line2 = encode_envelope(&env).unwrap();
    let env2 = decode_envelope(&line2).unwrap();
    assert!(matches!(env2, Envelope::Hello { .. }));
}

#[test]
fn roundtrip_event() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let line = encode_event("run-42", &event);
    let env = decode_envelope(&line).unwrap();
    match &env {
        Envelope::Event { ref_id, event: e } => {
            assert_eq!(ref_id, "run-42");
            assert!(
                matches!(&e.kind, AgentEventKind::AssistantMessage { text } if text == "hello")
            );
        }
        _ => panic!("expected Event envelope"),
    }
}

#[test]
fn roundtrip_final() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let line = encode_final("run-7", &receipt);
    let env = decode_envelope(&line).unwrap();
    match &env {
        Envelope::Final { ref_id, .. } => {
            assert_eq!(ref_id, "run-7");
        }
        _ => panic!("expected Final envelope"),
    }
}

#[test]
fn roundtrip_fatal() {
    let line = encode_fatal("run-8", "something went wrong");
    let env = decode_envelope(&line).unwrap();
    match &env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-8"));
            assert_eq!(error, "something went wrong");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal envelope"),
    }
}

#[test]
fn encode_envelope_ends_with_newline() {
    let hello = mock_hello("backend");
    let line = encode_envelope(&hello).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn decode_envelope_trims_whitespace() {
    let line = encode_fatal("r", "err");
    let padded = format!("  {}  ", line.trim());
    let env = decode_envelope(&padded).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn decode_envelope_invalid_json() {
    let result = decode_envelope("not json at all");
    assert!(result.is_err());
}

#[test]
fn encode_hello_unknown_capabilities_skipped() {
    let line = encode_hello("b", "1.0", &["streaming", "nonexistent_cap_xyz"]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        // Only "streaming" should have been inserted.
        assert_eq!(capabilities.len(), 1);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn encode_hello_empty_capabilities() {
    let line = encode_hello("b", "1.0", &[]);
    let env = decode_envelope(&line).unwrap();
    if let Envelope::Hello { capabilities, .. } = env {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn encode_hello_contains_contract_version() {
    let line = encode_hello("b", "1.0", &[]);
    assert!(line.contains(CONTRACT_VERSION));
}

// ========================================================================
// Validation helpers
// ========================================================================

#[test]
fn validate_hello_accepts_valid() {
    let hello = mock_hello("test");
    assert!(validate_hello(&hello).is_ok());
}

#[test]
fn validate_hello_rejects_non_hello() {
    let event = mock_event("run-1", "hi");
    let err = validate_hello(&event).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("event"), "error should mention 'event': {msg}");
}

#[test]
fn validate_hello_rejects_incompatible_version() {
    let hello = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: abp_core::BackendIdentity {
            id: "bad".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: abp_core::CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::default(),
    };
    let err = validate_hello(&hello).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("incompatible"),
        "error should mention incompatible: {msg}"
    );
}

#[test]
fn validate_ref_id_matches() {
    let event = mock_event("run-1", "text");
    assert!(validate_ref_id(&event, "run-1").is_ok());
}

#[test]
fn validate_ref_id_mismatch() {
    let event = mock_event("run-1", "text");
    let err = validate_ref_id(&event, "run-2").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("mismatch"),
        "error should mention mismatch: {msg}"
    );
}

#[test]
fn validate_ref_id_hello_always_passes() {
    let hello = mock_hello("b");
    assert!(validate_ref_id(&hello, "any-id").is_ok());
}

#[test]
fn validate_ref_id_fatal_with_ref() {
    let fatal = mock_fatal("run-1", "err");
    assert!(validate_ref_id(&fatal, "run-1").is_ok());
    assert!(validate_ref_id(&fatal, "run-2").is_err());
}

#[test]
fn validate_ref_id_fatal_without_ref() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    // No ref_id present → passes any expected value.
    assert!(validate_ref_id(&fatal, "run-1").is_ok());
}

#[test]
fn validate_ref_id_final() {
    let fin = mock_final("run-1");
    assert!(validate_ref_id(&fin, "run-1").is_ok());
    assert!(validate_ref_id(&fin, "run-wrong").is_err());
}

#[test]
fn validate_sequence_valid_hello_events_final() {
    let seq = vec![
        mock_hello("backend"),
        mock_event("r", "msg1"),
        mock_event("r", "msg2"),
        mock_final("r"),
    ];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_hello_and_fatal() {
    let seq = vec![mock_hello("b"), mock_fatal("r", "err")];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_empty_rejected() {
    let err = validate_sequence(&[]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("empty"), "error should mention empty: {msg}");
}

#[test]
fn validate_sequence_no_hello_first() {
    let seq = vec![mock_event("r", "text"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("hello"), "error should mention hello: {msg}");
}

#[test]
fn validate_sequence_no_terminal() {
    let seq = vec![mock_hello("b"), mock_event("r", "text")];
    let err = validate_sequence(&seq).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("final") || msg.contains("fatal") || msg.contains("terminal"),
        "error should mention terminal: {msg}"
    );
}

#[test]
fn validate_sequence_hello_only() {
    let seq = vec![mock_hello("b")];
    let err = validate_sequence(&seq).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("terminal") || msg.contains("at least"),
        "error should mention needing terminal: {msg}"
    );
}

#[test]
fn validate_sequence_duplicate_final() {
    let seq = vec![mock_hello("b"), mock_final("r"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("multiple") || msg.contains("terminal"),
        "error should mention duplicate terminal: {msg}"
    );
}

#[test]
fn validate_sequence_duplicate_hello() {
    let seq = vec![mock_hello("b"), mock_hello("b2"), mock_final("r")];
    let err = validate_sequence(&seq).unwrap_err();
    // Should be caught either as unexpected hello in middle or duplicate hello.
    assert!(err.to_string().contains("hello") || err.to_string().contains("unexpected"));
}

#[test]
fn validate_sequence_hello_final_minimal_valid() {
    let seq = vec![mock_hello("b"), mock_final("r")];
    assert!(validate_sequence(&seq).is_ok());
}

// ========================================================================
// Testing helpers
// ========================================================================

#[test]
fn mock_hello_produces_valid_envelope() {
    let env = mock_hello("test-backend");
    assert!(matches!(env, Envelope::Hello { .. }));
    assert!(validate_hello(&env).is_ok());
}

#[test]
fn mock_hello_roundtrip_jsonl() {
    let env = mock_hello("sidecar-x");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn mock_event_produces_valid_envelope() {
    let env = mock_event("run-1", "hello world");
    match &env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(
                matches!(&event.kind, AgentEventKind::AssistantMessage { text } if text == "hello world")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn mock_event_roundtrip_jsonl() {
    let env = mock_event("run-1", "test");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn mock_final_produces_valid_envelope() {
    let env = mock_final("run-2");
    match &env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-2");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn mock_final_roundtrip_jsonl() {
    let env = mock_final("r");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

#[test]
fn mock_fatal_produces_valid_envelope() {
    let env = mock_fatal("run-3", "oom");
    match &env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-3"));
            assert_eq!(error, "oom");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn mock_fatal_roundtrip_jsonl() {
    let env = mock_fatal("r", "err");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn mock_work_order_produces_valid_jsonl() {
    let line = mock_work_order("fix the tests");
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"run\""));
    assert!(line.contains("fix the tests"));
    let env = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(env, Envelope::Run { .. }));
}

#[test]
fn mock_work_order_unique_ids() {
    let line1 = mock_work_order("task 1");
    let line2 = mock_work_order("task 2");
    // Each call should produce a different work order id.
    assert_ne!(line1, line2);
}

// ========================================================================
// Error message quality
// ========================================================================

#[test]
fn error_messages_are_descriptive_hello_validation() {
    let event = mock_event("r", "t");
    let err = validate_hello(&event).unwrap_err();
    let msg = err.to_string();
    assert!(msg.len() > 10, "error message should be descriptive: {msg}");
    assert!(
        msg.contains("hello") || msg.contains("expected"),
        "should mention what was expected: {msg}"
    );
}

#[test]
fn error_messages_are_descriptive_ref_id_mismatch() {
    let event = mock_event("actual-id", "t");
    let err = validate_ref_id(&event, "expected-id").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("actual-id") && msg.contains("expected-id"),
        "should mention both actual and expected: {msg}"
    );
}

#[test]
fn error_messages_are_descriptive_sequence_violation() {
    let seq = vec![mock_event("r", "t")];
    let err = validate_sequence(&seq).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hello"),
        "should explain what is expected: {msg}"
    );
}

// ========================================================================
// Composite scenarios
// ========================================================================

#[test]
fn full_protocol_sequence_roundtrip() {
    // Simulate a full sidecar protocol session.
    let hello = mock_hello("my-backend");
    let work_order_line = mock_work_order("do stuff");
    let event1 = mock_event("run-1", "thinking...");
    let event2 = mock_event("run-1", "done!");
    let final_env = mock_final("run-1");

    // All should encode successfully.
    let hello_line = encode_envelope(&hello).unwrap();
    let event1_line = encode_envelope(&event1).unwrap();
    let event2_line = encode_envelope(&event2).unwrap();
    let final_line = encode_envelope(&final_env).unwrap();

    // All should decode back.
    assert!(matches!(
        decode_envelope(&hello_line).unwrap(),
        Envelope::Hello { .. }
    ));
    assert!(matches!(
        decode_envelope(&work_order_line).unwrap(),
        Envelope::Run { .. }
    ));
    assert!(matches!(
        decode_envelope(&event1_line).unwrap(),
        Envelope::Event { .. }
    ));
    assert!(matches!(
        decode_envelope(&event2_line).unwrap(),
        Envelope::Event { .. }
    ));
    assert!(matches!(
        decode_envelope(&final_line).unwrap(),
        Envelope::Final { .. }
    ));

    // The sequence (excluding the Run which comes from the control plane) should validate.
    let seq = vec![hello, event1, event2, final_env];
    assert!(validate_sequence(&seq).is_ok());
}

#[test]
fn validate_sequence_with_fatal_ending() {
    let seq = vec![
        mock_hello("b"),
        mock_event("r", "partial"),
        mock_fatal("r", "crashed"),
    ];
    assert!(validate_sequence(&seq).is_ok());
}
