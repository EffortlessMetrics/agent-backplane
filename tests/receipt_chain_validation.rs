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

//! Comprehensive integration tests for receipt chain validation across multiple runs.
//!
//! Covers: single receipt hashing, receipt chains, chain verification, tamper detection,
//! hash stability, null hash field, receipt store, query by hash, query by work order,
//! chain ordering, concurrent writes, large receipts, and receipt metadata.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport,
};
use abp_receipt::{
    ChainBuilder, ChainError, ReceiptBuilder, ReceiptChain, ReceiptValidator, TamperKind,
    canonicalize, compute_hash, verify_hash,
};
use abp_receipt_store::{
    InMemoryReceiptStore, ReceiptFilter, ReceiptIndex, ReceiptStore, validate_chain,
};

// ── Helpers ────────────────────────────────────────────────────────

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend.to_string(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

fn make_receipt_at(backend: &str, outcome: Outcome, ts: chrono::DateTime<Utc>) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.meta.started_at = ts;
    r.meta.finished_at = ts;
    r
}

fn make_hashed_receipt(backend: &str, outcome: Outcome) -> Receipt {
    let mut r = make_receipt(backend, outcome);
    r.receipt_sha256 = Some(abp_core::receipt_hash(&r).unwrap());
    r
}

fn make_hashed_receipt_at(backend: &str, outcome: Outcome, ts: chrono::DateTime<Utc>) -> Receipt {
    let mut r = make_receipt_at(backend, outcome, ts);
    r.receipt_sha256 = Some(abp_core::receipt_hash(&r).unwrap());
    r
}

fn make_builder_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .with_hash()
        .unwrap()
}

fn make_builder_receipt_at(backend: &str, outcome: Outcome, ts: chrono::DateTime<Utc>) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap()
}

// ────────────────────────────────────────────────────────────────────
// 1. Single Receipt Hashing
// ────────────────────────────────────────────────────────────────────

#[test]
fn hash_is_64_hex_chars() {
    let r = make_receipt("mock", Outcome::Complete);
    let h = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_is_deterministic() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_differs_for_different_receipts() {
    let r1 = make_receipt("mock-a", Outcome::Complete);
    let r2 = make_receipt("mock-b", Outcome::Failed);
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn hash_via_compute_hash_matches_core() {
    let r = make_receipt("mock", Outcome::Complete);
    let core_hash = abp_core::receipt_hash(&r).unwrap();
    let receipt_hash = compute_hash(&r).unwrap();
    assert_eq!(core_hash, receipt_hash);
}

#[test]
fn hash_changes_when_outcome_changes() {
    let mut r = make_receipt("mock", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.outcome = Outcome::Failed;
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn hash_changes_when_backend_changes() {
    let mut r = make_receipt("alpha", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.backend.id = "beta".to_string();
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn hash_changes_when_trace_added() {
    let mut r = make_receipt("mock", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();
    r.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    });
    let h2 = abp_core::receipt_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn builder_with_hash_produces_valid_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn builder_build_without_hash_has_none() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
}

// ────────────────────────────────────────────────────────────────────
// 2. Receipt Chain: Multiple receipts link via parent_hash
// ────────────────────────────────────────────────────────────────────

#[test]
fn chain_push_single() {
    let mut chain = ReceiptChain::new();
    let r = make_builder_receipt("mock", Outcome::Complete);
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(chain.parent_hash_at(0).is_none());
}

#[test]
fn chain_second_receipt_has_parent_hash() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("mock", Outcome::Complete, t1);
    let r1_hash = r1.receipt_sha256.clone().unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();

    let r2 = make_builder_receipt_at("mock", Outcome::Complete, t2);
    chain.push(r2).unwrap();

    assert_eq!(chain.parent_hash_at(1).unwrap(), r1_hash);
}

#[test]
fn chain_three_receipts_link_correctly() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_builder_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_builder_receipt_at("c", Outcome::Complete, t3);

    let r1_hash = r1.receipt_sha256.clone().unwrap();
    let r2_hash = r2.receipt_sha256.clone().unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    chain.push(r3).unwrap();

    assert!(chain.parent_hash_at(0).is_none());
    assert_eq!(chain.parent_hash_at(1).unwrap(), r1_hash);
    assert_eq!(chain.parent_hash_at(2).unwrap(), r2_hash);
}

#[test]
fn chain_builder_links_correctly() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("a", Outcome::Complete, t1);
    let r1_hash = r1.receipt_sha256.clone().unwrap();
    let r2 = make_builder_receipt_at("b", Outcome::Complete, t2);

    let chain = ChainBuilder::new()
        .append(r1)
        .unwrap()
        .append(r2)
        .unwrap()
        .build();

    assert_eq!(chain.len(), 2);
    assert_eq!(chain.parent_hash_at(1).unwrap(), r1_hash);
}

#[test]
fn chain_rejects_duplicate_run_id() {
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn chain_rejects_out_of_order_timestamps() {
    let t_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("a", Outcome::Complete, t_later);
    let r2 = make_builder_receipt_at("b", Outcome::Complete, t_earlier);

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::BrokenLink { .. }));
}

#[test]
fn chain_sequences_are_contiguous() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();
    chain
        .push(make_builder_receipt_at("c", Outcome::Complete, t3))
        .unwrap();

    assert_eq!(chain.sequence_at(0), Some(0));
    assert_eq!(chain.sequence_at(1), Some(1));
    assert_eq!(chain.sequence_at(2), Some(2));
}

// ────────────────────────────────────────────────────────────────────
// 3. Chain Verification
// ────────────────────────────────────────────────────────────────────

#[test]
fn verify_single_receipt_chain() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn verify_multi_receipt_chain() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Partial, t2))
        .unwrap();
    chain
        .push(make_builder_receipt_at("c", Outcome::Complete, t3))
        .unwrap();

    assert!(chain.verify().is_ok());
}

#[test]
fn verify_chain_full_checks_pass() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    assert!(chain.verify_chain().is_ok());
}

#[test]
fn verify_empty_chain_is_error() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn verify_chain_empty_is_error() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify_chain(), Err(ChainError::EmptyChain)));
}

#[test]
fn verify_chain_with_no_hash_receipts() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(make_receipt_at("a", Outcome::Complete, t1))
        .unwrap()
        .append(make_receipt_at("b", Outcome::Complete, t2))
        .unwrap()
        .build();

    // verify() passes since no hashes to check
    assert!(chain.verify().is_ok());
}

#[test]
fn validate_chain_store_empty() {
    let result = validate_chain(&[]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 0);
}

#[test]
fn validate_chain_store_single_hashed() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = validate_chain(&[r]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 1);
}

#[test]
fn validate_chain_store_multiple_chronological() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let result = validate_chain(&[
        make_hashed_receipt_at("a", Outcome::Complete, t1),
        make_hashed_receipt_at("b", Outcome::Partial, t2),
        make_hashed_receipt_at("c", Outcome::Failed, t3),
    ]);
    assert!(result.valid);
    assert_eq!(result.receipt_count, 3);
}

// ────────────────────────────────────────────────────────────────────
// 4. Tamper Detection
// ────────────────────────────────────────────────────────────────────

#[test]
fn tamper_detection_modified_outcome() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper after hashing

    let mut chain = ReceiptChain::new();
    let err = chain.push(r);
    assert!(matches!(err, Err(ChainError::HashMismatch { .. })));
}

#[test]
fn tamper_detection_modified_backend() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.backend.id = "tampered".to_string();

    let mut chain = ReceiptChain::new();
    let err = chain.push(r);
    assert!(matches!(err, Err(ChainError::HashMismatch { .. })));
}

#[test]
fn tamper_detection_modified_trace() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "injected".into(),
        },
        ext: None,
    });

    let mut chain = ReceiptChain::new();
    let err = chain.push(r);
    assert!(matches!(err, Err(ChainError::HashMismatch { .. })));
}

#[test]
fn tamper_detection_modified_hash_value() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".repeat(8));

    let mut chain = ReceiptChain::new();
    let err = chain.push(r);
    assert!(matches!(err, Err(ChainError::HashMismatch { .. })));
}

#[test]
fn detect_tampering_reports_hash_mismatch() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("a", Outcome::Complete, t1);
    let mut r2 = make_builder_receipt_at("b", Outcome::Complete, t2);

    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r1)
        .unwrap()
        .append({
            r2.outcome = Outcome::Failed; // tamper
            r2
        })
        .unwrap()
        .build();

    let evidence = chain.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(
        evidence
            .iter()
            .any(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
    );
}

#[test]
fn detect_tampering_clean_chain() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    let evidence = chain.detect_tampering();
    assert!(evidence.is_empty());
}

#[test]
fn tamper_breaks_validate_chain() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper
    let result = validate_chain(&[r]);
    assert!(!result.valid);
    assert!(result.errors[0].message.contains("hash mismatch"));
}

#[test]
fn verify_hash_rejects_tampered_receipt() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_accepts_no_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(verify_hash(&r));
}

// ────────────────────────────────────────────────────────────────────
// 5. Hash Stability
// ────────────────────────────────────────────────────────────────────

#[test]
fn hash_stable_across_serialize_deserialize() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn hash_stable_across_value_roundtrip() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = abp_core::receipt_hash(&r).unwrap();

    let value = serde_json::to_value(&r).unwrap();
    let r2: Receipt = serde_json::from_value(value).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn canonicalize_is_deterministic() {
    let r = make_receipt("mock", Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonicalize_ignores_existing_hash() {
    let mut r = make_receipt("mock", Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    r.receipt_sha256 = Some("some-hash".to_string());
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn hash_stable_with_usage_tokens() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_stable_with_artifacts() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "changes.patch".into(),
        })
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_changes_with_different_work_order_id() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .build();
    let h1 = compute_hash(&r1).unwrap();
    let h2 = compute_hash(&r2).unwrap();
    assert_ne!(h1, h2);
}

// ────────────────────────────────────────────────────────────────────
// 6. Null Hash Field
// ────────────────────────────────────────────────────────────────────

#[test]
fn receipt_sha256_is_none_before_hashing() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn builder_build_has_null_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn canonicalize_forces_null_hash() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("pre-existing".into());
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["receipt_sha256"].is_null());
}

#[test]
fn hash_same_regardless_of_stored_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("anything".to_string());
    let h2 = compute_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn hash_with_none_and_some_produce_same_result() {
    let r = make_receipt("mock", Outcome::Complete);
    let h_none = abp_core::receipt_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some(h_none.clone());
    let h_some = abp_core::receipt_hash(&r2).unwrap();

    assert_eq!(h_none, h_some);
}

// ────────────────────────────────────────────────────────────────────
// 7. Receipt Store
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn store_and_retrieve() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[tokio::test]
async fn store_rejects_duplicate() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    store.store(&r).await.unwrap();
    assert!(store.store(&r).await.is_err());
}

#[tokio::test]
async fn store_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get("nonexistent").await.unwrap().is_none());
}

#[tokio::test]
async fn store_delete_returns_true_when_exists() {
    let store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert!(store.delete(&id).await.unwrap());
}

#[tokio::test]
async fn store_delete_returns_false_when_missing() {
    let store = InMemoryReceiptStore::new();
    assert!(!store.delete("nonexistent").await.unwrap());
}

#[tokio::test]
async fn store_count_reflects_operations() {
    let store = InMemoryReceiptStore::new();
    assert_eq!(store.count().await.unwrap(), 0);

    let r = make_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id.to_string();
    store.store(&r).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);

    store.delete(&id).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn store_list_all() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn store_preserves_hash_through_roundtrip() {
    let store = InMemoryReceiptStore::new();
    let r = make_builder_receipt("mock", Outcome::Complete);
    let original_hash = r.receipt_sha256.clone();
    let id = r.meta.run_id.to_string();

    store.store(&r).await.unwrap();
    let got = store.get(&id).await.unwrap().unwrap();
    assert_eq!(got.receipt_sha256, original_hash);
}

// ────────────────────────────────────────────────────────────────────
// 8. Query by Hash
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn find_receipt_by_hash_via_list_scan() {
    let store = InMemoryReceiptStore::new();
    let r1 = make_hashed_receipt("a", Outcome::Complete);
    let r2 = make_hashed_receipt("b", Outcome::Failed);
    let target_hash = r1.receipt_sha256.clone().unwrap();

    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let found: Vec<_> = all
        .iter()
        .filter(|r| r.receipt_sha256.as_deref() == Some(&target_hash))
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].backend.id, "a");
}

#[tokio::test]
async fn unique_hashes_for_different_receipts_in_store() {
    let store = InMemoryReceiptStore::new();
    let r1 = make_hashed_receipt("a", Outcome::Complete);
    let r2 = make_hashed_receipt("b", Outcome::Failed);

    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);

    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let hashes: Vec<_> = all
        .iter()
        .filter_map(|r| r.receipt_sha256.as_ref())
        .collect();
    assert_eq!(hashes.len(), 2);
    assert_ne!(hashes[0], hashes[1]);
}

// ────────────────────────────────────────────────────────────────────
// 9. Query by Work Order
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn find_receipt_by_work_order_id() {
    let store = InMemoryReceiptStore::new();
    let wo_id = Uuid::new_v4();
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.work_order_id = wo_id;
    store.store(&r).await.unwrap();

    // Also store an unrelated receipt.
    store
        .store(&make_receipt("other", Outcome::Failed))
        .await
        .unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let found: Vec<_> = all
        .iter()
        .filter(|r| r.meta.work_order_id == wo_id)
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].backend.id, "mock");
}

#[tokio::test]
async fn multiple_receipts_same_work_order() {
    let store = InMemoryReceiptStore::new();
    let wo_id = Uuid::new_v4();

    let mut r1 = make_receipt("run1", Outcome::Failed);
    r1.meta.work_order_id = wo_id;
    let mut r2 = make_receipt("run2", Outcome::Complete);
    r2.meta.work_order_id = wo_id;

    store.store(&r1).await.unwrap();
    store.store(&r2).await.unwrap();

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    let found: Vec<_> = all
        .iter()
        .filter(|r| r.meta.work_order_id == wo_id)
        .collect();
    assert_eq!(found.len(), 2);
}

// ────────────────────────────────────────────────────────────────────
// 10. Chain Ordering
// ────────────────────────────────────────────────────────────────────

#[test]
fn chain_ordering_maintained_on_iteration() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("first", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("second", Outcome::Complete, t2))
        .unwrap();
    chain
        .push(make_builder_receipt_at("third", Outcome::Complete, t3))
        .unwrap();

    let backends: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(backends, vec!["first", "second", "third"]);
}

#[test]
fn chain_latest_returns_last_pushed() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("first", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("last", Outcome::Complete, t2))
        .unwrap();

    assert_eq!(chain.latest().unwrap().backend.id, "last");
}

#[test]
fn chain_get_by_index() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("zero", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("one", Outcome::Complete, t2))
        .unwrap();

    assert_eq!(chain.get(0).unwrap().backend.id, "zero");
    assert_eq!(chain.get(1).unwrap().backend.id, "one");
    assert!(chain.get(2).is_none());
}

#[test]
fn validate_chain_detects_wrong_order() {
    let t_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

    let result = validate_chain(&[
        make_hashed_receipt_at("a", Outcome::Complete, t_later),
        make_hashed_receipt_at("b", Outcome::Complete, t_earlier),
    ]);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.message.contains("earlier")));
}

#[test]
fn chain_same_timestamp_is_allowed() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, ts))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, ts))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_find_gaps_detects_sequence_gaps() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = make_builder_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_builder_receipt_at("b", Outcome::Complete, t2);

    let chain = ChainBuilder::new()
        .append_with_sequence(r1, 0)
        .unwrap()
        .append_with_sequence(r2, 5) // gap: 1..4 missing
        .unwrap()
        .build();

    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

#[test]
fn chain_no_gaps_when_contiguous() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    assert!(chain.find_gaps().is_empty());
}

// ────────────────────────────────────────────────────────────────────
// 11. Concurrent Writes
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn concurrent_writes_to_store() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();

    for _ in 0..30 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("concurrent", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 30);
}

#[tokio::test]
async fn concurrent_reads_and_writes() {
    let store = Arc::new(InMemoryReceiptStore::new());

    // Pre-populate.
    for _ in 0..5 {
        store
            .store(&make_receipt("init", Outcome::Complete))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();

    // Writers.
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = make_receipt("writer", Outcome::Complete);
            s.store(&r).await.unwrap();
        }));
    }

    // Readers.
    for _ in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let _ = s.list(ReceiptFilter::default()).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 15);
}

#[tokio::test]
async fn concurrent_writes_unique_ids() {
    let store = Arc::new(InMemoryReceiptStore::new());
    let mut handles = Vec::new();

    for i in 0..20u32 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let r = ReceiptBuilder::new(format!("backend-{i}"))
                .outcome(Outcome::Complete)
                .build();
            s.store(&r).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let all = store.list(ReceiptFilter::default()).await.unwrap();
    assert_eq!(all.len(), 20);
}

// ────────────────────────────────────────────────────────────────────
// 12. Large Receipts
// ────────────────────────────────────────────────────────────────────

#[test]
fn large_receipt_with_many_events_hashes_correctly() {
    let now = Utc::now();
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: format!("message {i}"),
            },
            ext: None,
        })
        .collect();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 100);
}

#[test]
fn large_receipt_with_mixed_events() {
    let now = Utc::now();
    let mut events: Vec<AgentEvent> = Vec::new();

    for i in 0..50 {
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("use_{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"arg": i}),
            },
            ext: None,
        });
        events.push(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("use_{i}")),
                output: serde_json::json!({"result": i * 2}),
                is_error: false,
            },
            ext: None,
        });
    }

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 100);
}

#[test]
fn large_receipt_hash_is_deterministic() {
    let now = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let events: Vec<AgentEvent> = (0..200)
        .map(|i| AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            },
            ext: None,
        })
        .collect();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(now)
        .finished_at(now)
        .events(events)
        .build();

    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_with_many_artifacts() {
    let artifacts: Vec<ArtifactRef> = (0..50)
        .map(|i| ArtifactRef {
            kind: "patch".into(),
            path: format!("patch_{i}.diff"),
        })
        .collect();

    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.artifacts = artifacts;
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());

    assert!(verify_hash(&r));
    assert_eq!(r.artifacts.len(), 50);
}

// ────────────────────────────────────────────────────────────────────
// 13. Receipt Metadata
// ────────────────────────────────────────────────────────────────────

#[test]
fn receipt_metadata_timestamps() {
    let start = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 1, 1, 12, 5, 0).unwrap();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(end)
        .build();

    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
    assert_eq!(r.meta.duration_ms, 300_000); // 5 minutes
}

#[test]
fn receipt_metadata_duration_from_builder() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .duration(Duration::from_secs(10))
        .build();

    assert_eq!(r.meta.duration_ms, 10_000);
}

#[test]
fn receipt_metadata_contract_version() {
    let r = make_receipt("mock", Outcome::Complete);
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_metadata_work_order_id_set() {
    let wo_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn receipt_metadata_run_id_unique() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = make_receipt("mock", Outcome::Complete);
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[test]
fn receipt_metadata_explicit_run_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(id)
        .build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn receipt_usage_tokens() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(1000, 500)
        .build();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(500));
}

#[test]
fn receipt_backend_version() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .backend_version("2.1.0")
        .adapter_version("0.5.0")
        .build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn receipt_event_count_in_trace() {
    let now = Utc::now();
    let events: Vec<AgentEvent> = (0..7)
        .map(|i| AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: format!("msg {i}"),
            },
            ext: None,
        })
        .collect();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .events(events)
        .build();
    assert_eq!(r.trace.len(), 7);
}

// ────────────────────────────────────────────────────────────────────
// Chain Summary Statistics
// ────────────────────────────────────────────────────────────────────

#[test]
fn chain_summary_basic_stats() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Failed, t2))
        .unwrap();
    chain
        .push(make_builder_receipt_at("c", Outcome::Partial, t3))
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 3);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.partial_count, 1);
    assert!(summary.all_hashes_valid);
    assert_eq!(summary.gap_count, 0);
}

#[test]
fn chain_summary_backends() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("alpha", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("beta", Outcome::Complete, t2))
        .unwrap();
    chain
        .push(make_builder_receipt_at("alpha", Outcome::Complete, t3))
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.backends.len(), 2);
    assert!(summary.backends.contains(&"alpha".to_string()));
    assert!(summary.backends.contains(&"beta".to_string()));
}

#[test]
fn chain_summary_token_counts() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("a")
        .outcome(Outcome::Complete)
        .started_at(t1)
        .finished_at(t1)
        .usage_tokens(100, 50)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .started_at(t2)
        .finished_at(t2)
        .usage_tokens(200, 75)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.total_input_tokens, 300);
    assert_eq!(summary.total_output_tokens, 125);
}

#[test]
fn chain_summary_time_range() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.first_started_at, Some(t1));
    assert_eq!(summary.last_finished_at, Some(t2));
}

// ────────────────────────────────────────────────────────────────────
// Receipt Validator
// ────────────────────────────────────────────────────────────────────

#[test]
fn validator_accepts_valid_receipt() {
    let v = ReceiptValidator::new();
    let r = make_builder_receipt("mock", Outcome::Complete);
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_rejects_bad_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.meta.contract_version = "bad/v999".to_string();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_rejects_tampered_hash() {
    let v = ReceiptValidator::new();
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn validator_rejects_empty_backend_id() {
    let v = ReceiptValidator::new();
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.backend.id = String::new();
    // Recompute hash for the tampered receipt so hash check passes.
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

// ────────────────────────────────────────────────────────────────────
// Receipt Index
// ────────────────────────────────────────────────────────────────────

#[test]
fn index_by_backend_query() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("alpha", Outcome::Complete);
    let r2 = make_receipt("beta", Outcome::Complete);
    let r1_id = r1.meta.run_id.to_string();

    idx.insert(&r1);
    idx.insert(&r2);

    let alpha_ids = idx.by_backend("alpha");
    assert_eq!(alpha_ids.len(), 1);
    assert!(alpha_ids.contains(&r1_id));
}

#[test]
fn index_by_outcome_query() {
    let mut idx = ReceiptIndex::new();
    let r1 = make_receipt("a", Outcome::Complete);
    let r2 = make_receipt("b", Outcome::Failed);
    let r3 = make_receipt("c", Outcome::Complete);

    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    assert_eq!(idx.by_outcome(&Outcome::Complete).len(), 2);
    assert_eq!(idx.by_outcome(&Outcome::Failed).len(), 1);
}

#[test]
fn index_by_time_range_query() {
    let mut idx = ReceiptIndex::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let r1 = make_receipt_at("a", Outcome::Complete, t1);
    let r2 = make_receipt_at("b", Outcome::Complete, t2);
    let r3 = make_receipt_at("c", Outcome::Complete, t3);

    idx.insert(&r1);
    idx.insert(&r2);
    idx.insert(&r3);

    let mid_range = idx.by_time_range(
        Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
    );
    assert_eq!(mid_range.len(), 1);
}

#[test]
fn index_remove_cleans_up() {
    let mut idx = ReceiptIndex::new();
    let r = make_receipt("mock", Outcome::Complete);
    idx.insert(&r);
    assert_eq!(idx.len(), 1);

    idx.remove(&r);
    assert!(idx.is_empty());
    assert!(idx.by_backend("mock").is_empty());
}

// ────────────────────────────────────────────────────────────────────
// Store Filtering
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn store_filter_by_outcome() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("a", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("b", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("c", Outcome::Partial))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn store_filter_by_backend() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("beta", Outcome::Complete))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("beta".into()),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "beta");
}

#[tokio::test]
async fn store_filter_by_time_range() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt_at("early", Outcome::Complete, t1))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("mid", Outcome::Complete, t2))
        .await
        .unwrap();
    store
        .store(&make_receipt_at("late", Outcome::Complete, t3))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        time_range: Some((
            Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap(),
        )),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "mid");
}

#[tokio::test]
async fn store_filter_combined() {
    let store = InMemoryReceiptStore::new();
    store
        .store(&make_receipt("alpha", Outcome::Complete))
        .await
        .unwrap();
    store
        .store(&make_receipt("alpha", Outcome::Failed))
        .await
        .unwrap();
    store
        .store(&make_receipt("beta", Outcome::Failed))
        .await
        .unwrap();

    let filter = ReceiptFilter {
        backend: Some("alpha".into()),
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
    assert_eq!(results[0].outcome, Outcome::Failed);
}

// ────────────────────────────────────────────────────────────────────
// Chain Serialization Roundtrip
// ────────────────────────────────────────────────────────────────────

#[test]
fn chain_serializes_and_deserializes() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.len(), 2);
    assert!(deserialized.verify().is_ok());
}

#[test]
fn chain_roundtrip_preserves_hashes() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();

    let mut chain = ReceiptChain::new();
    chain
        .push(make_builder_receipt_at("a", Outcome::Complete, t1))
        .unwrap();
    chain
        .push(make_builder_receipt_at("b", Outcome::Complete, t2))
        .unwrap();

    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();

    for i in 0..2 {
        assert_eq!(
            chain.get(i).unwrap().receipt_sha256,
            deserialized.get(i).unwrap().receipt_sha256
        );
    }
}

// ────────────────────────────────────────────────────────────────────
// Auditor Integration
// ────────────────────────────────────────────────────────────────────

#[test]
fn auditor_clean_batch() {
    let auditor = abp_receipt::ReceiptAuditor::new();
    let r1 = make_builder_receipt("a", Outcome::Complete);
    let r2 = make_builder_receipt("b", Outcome::Complete);
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report.is_clean());
    assert_eq!(report.total, 2);
    assert_eq!(report.valid, 2);
}

#[test]
fn auditor_detects_tampered_receipt() {
    let auditor = abp_receipt::ReceiptAuditor::new();
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_detects_duplicate_run_ids() {
    let auditor = abp_receipt::ReceiptAuditor::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report.is_clean());
}

// ────────────────────────────────────────────────────────────────────
// Verification Integration
// ────────────────────────────────────────────────────────────────────

#[test]
fn verify_receipt_all_checks_pass() {
    let r = make_builder_receipt("mock", Outcome::Complete);
    let result = abp_receipt::verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.hash_valid);
    assert!(result.contract_valid);
    assert!(result.timestamps_valid);
    assert!(result.outcome_consistent);
}

#[test]
fn verify_receipt_detects_bad_hash() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed;
    let result = abp_receipt::verify_receipt(&r);
    assert!(!result.is_verified());
    assert!(!result.hash_valid);
}

#[test]
fn verify_receipt_detects_bad_contract() {
    let mut r = make_builder_receipt("mock", Outcome::Complete);
    r.meta.contract_version = "wrong".to_string();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    let result = abp_receipt::verify_receipt(&r);
    assert!(!result.contract_valid);
}
