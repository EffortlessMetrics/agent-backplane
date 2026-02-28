// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the envelope builder module.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::builder::{BuilderError, EnvelopeBuilder};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    }
}

fn sample_receipt() -> abp_core::Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build()
}

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("do something").build()
}

// ---------------------------------------------------------------------------
// Hello
// ---------------------------------------------------------------------------

#[test]
fn build_hello_envelope() {
    let env = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn hello_missing_backend_returns_error() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(err, BuilderError::MissingField("backend"));
    assert!(err.to_string().contains("backend"));
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = EnvelopeBuilder::hello()
        .backend("x")
        .build()
        .unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let env = EnvelopeBuilder::hello()
        .backend("x")
        .capabilities(caps)
        .build()
        .unwrap();

    match env {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn hello_sets_contract_version() {
    let env = EnvelopeBuilder::hello()
        .backend("x")
        .build()
        .unwrap();
    match env {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, abp_core::CONTRACT_VERSION),
        _ => panic!("wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

#[test]
fn build_run_envelope() {
    let wo = sample_work_order();
    let id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match env {
        Envelope::Run {
            id: envelope_id, ..
        } => assert_eq!(envelope_id, id),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn run_with_custom_ref_id() {
    let wo = sample_work_order();
    let env = EnvelopeBuilder::run(wo)
        .ref_id("custom-123")
        .build()
        .unwrap();
    match env {
        Envelope::Run { id, .. } => assert_eq!(id, "custom-123"),
        _ => panic!("wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

#[test]
fn build_event_envelope() {
    let env = EnvelopeBuilder::event(sample_event())
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn event_missing_ref_id_returns_error() {
    let err = EnvelopeBuilder::event(sample_event())
        .build()
        .unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

#[test]
fn event_ref_id_set_correctly() {
    let env = EnvelopeBuilder::event(sample_event())
        .ref_id("abc")
        .build()
        .unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "abc"),
        _ => panic!("wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// Final
// ---------------------------------------------------------------------------

#[test]
fn build_final_envelope() {
    let env = EnvelopeBuilder::final_receipt(sample_receipt())
        .ref_id("run-1")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Final { .. }));
}

#[test]
fn final_missing_ref_id_returns_error() {
    let err = EnvelopeBuilder::final_receipt(sample_receipt())
        .build()
        .unwrap_err();
    assert_eq!(err, BuilderError::MissingField("ref_id"));
}

// ---------------------------------------------------------------------------
// Fatal
// ---------------------------------------------------------------------------

#[test]
fn build_fatal_envelope() {
    let env = EnvelopeBuilder::fatal("boom").build().unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn fatal_with_ref_id() {
    let env = EnvelopeBuilder::fatal("err")
        .ref_id("run-99")
        .build()
        .unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-99")),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn fatal_with_code() {
    // code is stored but not part of the envelope wire format (yet)
    let env = EnvelopeBuilder::fatal("err")
        .code("E_OOM")
        .build()
        .unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

// ---------------------------------------------------------------------------
// Roundtrip
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_hello_serialize_deserialize() {
    let env = EnvelopeBuilder::hello()
        .backend("rt-test")
        .version("0.1")
        .mode(ExecutionMode::Passthrough)
        .build()
        .unwrap();

    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    match decoded {
        Envelope::Hello { backend, mode, .. } => {
            assert_eq!(backend.id, "rt-test");
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn roundtrip_fatal_serialize_deserialize() {
    let env = EnvelopeBuilder::fatal("kaboom")
        .ref_id("r-1")
        .build()
        .unwrap();

    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    match decoded {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("r-1"));
            assert_eq!(error, "kaboom");
        }
        _ => panic!("wrong variant"),
    }
}

// ---------------------------------------------------------------------------
// Builder chaining
// ---------------------------------------------------------------------------

#[test]
fn builder_chaining_hello() {
    // Verify all methods can be chained in a single expression.
    let result = EnvelopeBuilder::hello()
        .backend("chain-test")
        .version("1.0")
        .adapter_version("0.5")
        .mode(ExecutionMode::Mapped)
        .capabilities(CapabilityManifest::new())
        .build();
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// BuilderError display
// ---------------------------------------------------------------------------

#[test]
fn builder_error_display() {
    let err = BuilderError::MissingField("foo");
    assert_eq!(err.to_string(), "missing required field: foo");
}
