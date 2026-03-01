// SPDX-License-Identifier: MIT OR Apache-2.0
//! Negative and edge-case tests for abp-host sidecar handshake types.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────

fn test_capabilities() -> CapabilityManifest {
    BTreeMap::new()
}

// ── 1. SidecarHello with empty backend_id ───────────────────────────

#[test]
fn hello_with_empty_backend_id_round_trips() {
    let backend = BackendIdentity {
        id: "".into(),
        backend_version: None,
        adapter_version: None,
    };
    let env = Envelope::hello(backend, test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "");
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn hello_with_empty_backend_id_preserves_in_json() {
    let backend = BackendIdentity {
        id: "".into(),
        backend_version: None,
        adapter_version: None,
    };
    let env = Envelope::hello(backend, test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r#""id":"""#));
}

// ── 2. SidecarHello with mismatched contract_version ────────────────

#[test]
fn hello_with_wrong_contract_version_round_trips() {
    // The protocol layer does not reject mismatched versions on decode;
    // that's the host's responsibility at a higher level.
    let env = Envelope::Hello {
        contract_version: "abp/v999.0".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "abp/v999.0");
            assert_ne!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn hello_with_empty_contract_version_round_trips() {
    let env = Envelope::Hello {
        contract_version: "".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: ExecutionMode::default(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "");
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn hello_version_mismatch_is_detectable() {
    let env = Envelope::Hello {
        contract_version: "abp/v0.2".into(),
        backend: BackendIdentity {
            id: "future-sidecar".into(),
            backend_version: Some("2.0".into()),
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: ExecutionMode::Mapped,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            // A host implementation should check this
            assert_ne!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}
