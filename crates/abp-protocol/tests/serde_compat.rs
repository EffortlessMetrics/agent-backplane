// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde compatibility and backwards-compatibility tests for abp-protocol envelope types.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn minimal_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn minimal_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test").build()
}

// ===========================================================================
// 1. Tag field — discriminator is "t", not "type"
// ===========================================================================

#[test]
fn envelope_discriminator_is_t() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&env).unwrap();
    assert!(
        v.get("t").is_some(),
        "Envelope must use \"t\" as tag field, got: {v}"
    );
    assert!(
        v.get("type").is_none(),
        "Envelope must NOT use \"type\" as tag field"
    );
}

#[test]
fn envelope_tag_values_are_snake_case() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let v = serde_json::to_value(&hello).unwrap();
    assert_eq!(v["t"], "hello");

    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&fatal).unwrap();
    assert_eq!(v["t"], "fatal");
}

// ===========================================================================
// 2. All envelope variants round-trip through serde
// ===========================================================================

#[test]
fn roundtrip_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    assert!(matches!(back, Envelope::Hello { .. }));
}

#[test]
fn roundtrip_run() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: minimal_work_order(),
    };
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    assert!(matches!(back, Envelope::Run { .. }));
}

#[test]
fn roundtrip_event() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        },
    };
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    assert!(matches!(back, Envelope::Event { .. }));
}

#[test]
fn roundtrip_final() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: minimal_receipt(),
    };
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    assert!(matches!(back, Envelope::Final { .. }));
}

#[test]
fn roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    assert!(matches!(back, Envelope::Fatal { .. }));
}

#[test]
fn roundtrip_fatal_null_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "no ref".into(),
        error_code: None,
    };
    let json_str = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json_str).unwrap();
    match back {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "no ref");
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 3. Stripped fields — extra fields in envelope JSON are tolerated
// ===========================================================================

#[test]
fn extra_fields_in_fatal_tolerated() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","debug_info":"extra"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn extra_fields_in_hello_tolerated() {
    let json = json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "test" },
        "capabilities": {},
        "mode": "mapped",
        "extensions": { "v2_feature": true }
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn extra_fields_in_event_tolerated() {
    let json = json!({
        "t": "event",
        "ref_id": "run-1",
        "event": {
            "ts": "2025-01-01T00:00:00Z",
            "type": "warning",
            "message": "test"
        },
        "sequence_number": 42
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

// ===========================================================================
// 4. Minimal valid envelopes — smallest possible valid JSON
// ===========================================================================

#[test]
fn minimal_fatal() {
    let json = r#"{"t":"fatal","error":"x"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "x");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn minimal_hello() {
    // mode is omitted — should default to Mapped.
    let json = json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "x" },
        "capabilities": {}
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    match env {
        Envelope::Hello { mode, backend, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
            assert_eq!(backend.id, "x");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn minimal_event() {
    let json = json!({
        "t": "event",
        "ref_id": "r",
        "event": {
            "ts": "2025-01-01T00:00:00Z",
            "type": "error",
            "message": "e"
        }
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn minimal_run() {
    let wo = minimal_work_order();
    let json = json!({
        "t": "run",
        "id": "r",
        "work_order": serde_json::to_value(&wo).unwrap()
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    assert!(matches!(env, Envelope::Run { .. }));
}

#[test]
fn minimal_final() {
    let receipt = minimal_receipt();
    let json = json!({
        "t": "final",
        "ref_id": "r",
        "receipt": serde_json::to_value(&receipt).unwrap()
    });
    let env: Envelope = serde_json::from_value(json).unwrap();
    assert!(matches!(env, Envelope::Final { .. }));
}

// ===========================================================================
// Additional: JsonlCodec encode/decode preserves tag
// ===========================================================================

#[test]
fn jsonl_codec_preserves_tag_field() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"fatal\""), "encoded line: {line}");

    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}
