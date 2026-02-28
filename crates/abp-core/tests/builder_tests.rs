// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::validate::validate_receipt;
use abp_core::*;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

#[test]
fn build_minimal_receipt() {
    let receipt = ReceiptBuilder::new("mock").build();

    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.receipt_sha256.is_none());
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
}

#[test]
fn build_receipt_with_all_fields() {
    let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let wo_id = Uuid::new_v4();

    let event = AgentEvent {
        ts: start,
        kind: AgentEventKind::RunStarted {
            message: "hello".into(),
        },
        ext: None,
    };

    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };

    let receipt = ReceiptBuilder::new("test-backend")
        .backend_id("overridden")
        .outcome(Outcome::Partial)
        .started_at(start)
        .finished_at(end)
        .work_order_id(wo_id)
        .add_trace_event(event)
        .add_artifact(artifact)
        .backend_version("1.0")
        .adapter_version("0.1")
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        })
        .verification(VerificationReport {
            harness_ok: true,
            ..Default::default()
        })
        .build();

    assert_eq!(receipt.backend.id, "overridden");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.started_at, start);
    assert_eq!(receipt.meta.finished_at, end);
    assert_eq!(receipt.meta.duration_ms, 5000);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.trace.len(), 1);
    assert_eq!(receipt.artifacts.len(), 1);
    assert!(receipt.verification.harness_ok);
}

#[test]
fn build_receipt_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex digest
}

#[test]
fn builder_produces_valid_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    validate_receipt(&receipt).expect("receipt should be valid");
}

#[test]
fn builder_default_values_are_sensible() {
    let receipt = ReceiptBuilder::new("test").build();

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.meta.work_order_id, Uuid::nil());
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert!(receipt.backend.backend_version.is_none());
    assert!(receipt.backend.adapter_version.is_none());
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
    assert!(receipt.receipt_sha256.is_none());
    assert!(receipt.capabilities.is_empty());
    // started_at <= finished_at
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    assert_eq!(receipt.meta.duration_ms, 0);
}

#[test]
fn chain_multiple_trace_events() {
    let now = Utc::now();
    let make_event = |msg: &str| AgentEvent {
        ts: now,
        kind: AgentEventKind::AssistantMessage {
            text: msg.to_string(),
        },
        ext: None,
    };

    let receipt = ReceiptBuilder::new("mock")
        .add_trace_event(make_event("first"))
        .add_trace_event(make_event("second"))
        .add_trace_event(make_event("third"))
        .build();

    assert_eq!(receipt.trace.len(), 3);
}
