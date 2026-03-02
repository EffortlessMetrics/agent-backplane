// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! End-to-end receipt chain tests proving work orders flow through the system,
//! produce receipts with valid hashes, and receipt chains maintain integrity.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrderBuilder, canonical_json, receipt_hash,
    sha256_hex,
};
use abp_receipt::{
    ChainError, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash, verify_hash,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal receipt with controlled timestamps.
fn receipt_at(backend: &str, start: DateTime<Utc>, dur_ms: i64, outcome: Outcome) -> Receipt {
    let finish = start + Duration::milliseconds(dur_ms);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .build()
}

/// Build a hashed receipt at a given offset from a base time.
fn hashed_receipt(backend: &str, offset_secs: i64, outcome: Outcome) -> Receipt {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let start = base + Duration::seconds(offset_secs);
    let finish = start + Duration::milliseconds(100);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap()
}

/// Build a receipt with a fixed run_id and work_order_id for determinism tests.
fn deterministic_receipt(
    run_id: Uuid,
    wo_id: Uuid,
    start: DateTime<Utc>,
    finish: DateTime<Utc>,
) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo_id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: start,
            finished_at: finish,
            duration_ms: (finish - start).num_milliseconds().max(0) as u64,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("0.1".into()),
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. Single work order → receipt flow with valid hash
// ===========================================================================

#[test]
fn single_receipt_has_valid_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn single_receipt_hash_verifies() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_without_hash_verifies_trivially() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_hash_contains_contract_version() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_with_hash_has_valid_run_id() {
    let r = ReceiptBuilder::new("mock").build();
    assert_ne!(r.meta.run_id, Uuid::nil());
}

#[test]
fn receipt_builder_sets_backend_identity() {
    let r = ReceiptBuilder::new("test-backend")
        .backend_version("2.0")
        .adapter_version("1.0")
        .build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0"));
}

#[test]
fn work_order_id_round_trips_through_receipt() {
    let wo = WorkOrderBuilder::new("test task").build();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_eq!(r.meta.work_order_id, wo.id);
    assert!(verify_hash(&r));
}

// ===========================================================================
// 2. Receipt hash determinism (same inputs → same hash)
// ===========================================================================

#[test]
fn hash_determinism_same_receipt() {
    let run_id = Uuid::nil();
    let wo_id = Uuid::nil();
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);

    let r1 = deterministic_receipt(run_id, wo_id, start, finish);
    let r2 = deterministic_receipt(run_id, wo_id, start, finish);

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_determinism_via_compute_hash() {
    let run_id = Uuid::nil();
    let wo_id = Uuid::nil();
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);

    let r = deterministic_receipt(run_id, wo_id, start, finish);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_determinism_across_core_and_receipt_crates() {
    let run_id = Uuid::nil();
    let wo_id = Uuid::nil();
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);

    let r = deterministic_receipt(run_id, wo_id, start, finish);
    let core_hash = receipt_hash(&r).unwrap();
    let receipt_hash_val = compute_hash(&r).unwrap();
    assert_eq!(core_hash, receipt_hash_val);
}

#[test]
fn hash_determinism_1000_iterations() {
    let run_id = Uuid::nil();
    let wo_id = Uuid::nil();
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(100);
    let r = deterministic_receipt(run_id, wo_id, start, finish);

    let expected = receipt_hash(&r).unwrap();
    for _ in 0..1000 {
        assert_eq!(receipt_hash(&r).unwrap(), expected);
    }
}

#[test]
fn canonical_json_is_deterministic() {
    let r = deterministic_receipt(
        Uuid::nil(),
        Uuid::nil(),
        DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .to_utc(),
        DateTime::parse_from_rfc3339("2025-01-01T00:00:01Z")
            .unwrap()
            .to_utc(),
    );
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
}

// ===========================================================================
// 3. Receipt hash changes when any field changes
// ===========================================================================

#[test]
fn hash_changes_with_different_backend_id() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.backend.id = "other-backend".into();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_outcome() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_work_order_id() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.meta.work_order_id = Uuid::new_v4();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_run_id() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.meta.run_id = Uuid::new_v4();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_timestamps() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.meta.started_at += Duration::seconds(1);

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_duration() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.meta.duration_ms = 9999;

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_mode() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.mode = ExecutionMode::Passthrough;

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_usage_raw() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.usage_raw = json!({"tokens": 42});

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_trace() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    }));

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_artifacts() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "fix.patch".into(),
    });

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_verification() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.verification.harness_ok = true;
    r2.verification.git_diff = Some("diff --git a/foo".into());

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_capabilities() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_contract_version() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.meta.contract_version = "abp/v99".into();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_backend_version() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.backend.backend_version = Some("9.9.9".into());

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_different_usage_normalized() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(42);
    let r1 = deterministic_receipt(Uuid::nil(), Uuid::nil(), start, finish);

    let mut r2 = r1.clone();
    r2.usage.input_tokens = Some(500);

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 4. Receipt chain: WO1→R1→WO2(ref R1)→R2 with hash chaining
// ===========================================================================

#[test]
fn chain_two_receipts_linked_by_work_order() {
    let wo1 = WorkOrderBuilder::new("step 1").build();
    let r1 = ReceiptBuilder::new("mock")
        .work_order_id(wo1.id)
        .outcome(Outcome::Complete)
        .started_at(
            DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .unwrap()
                .to_utc(),
        )
        .finished_at(
            DateTime::parse_from_rfc3339("2025-01-01T00:00:01Z")
                .unwrap()
                .to_utc(),
        )
        .with_hash()
        .unwrap();

    let wo2 = WorkOrderBuilder::new("step 2").build();
    let r2 = ReceiptBuilder::new("mock")
        .work_order_id(wo2.id)
        .outcome(Outcome::Complete)
        .started_at(
            DateTime::parse_from_rfc3339("2025-01-01T00:00:02Z")
                .unwrap()
                .to_utc(),
        )
        .finished_at(
            DateTime::parse_from_rfc3339("2025-01-01T00:00:03Z")
                .unwrap()
                .to_utc(),
        )
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_five_step_workflow() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let r = hashed_receipt("mock", i * 10, Outcome::Complete);
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_latest_is_last_pushed() {
    let mut chain = ReceiptChain::new();
    let r1 = hashed_receipt("mock", 0, Outcome::Complete);
    let r2 = hashed_receipt("mock", 10, Outcome::Complete);
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[test]
fn chain_rejects_tampered_hash() {
    let mut r = hashed_receipt("mock", 0, Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".repeat(8));

    let mut chain = ReceiptChain::new();
    let err = chain.push(r).unwrap_err();
    assert!(matches!(err, ChainError::HashMismatch { index: 0 }));
}

#[test]
fn chain_rejects_duplicate_run_id() {
    let r1 = hashed_receipt("mock", 0, Outcome::Complete);
    let mut r2 = r1.clone();
    // Must have different start time but same run_id
    r2.meta.started_at = r1.meta.started_at + Duration::seconds(5);
    r2.meta.finished_at = r2.meta.started_at + Duration::milliseconds(100);
    r2.receipt_sha256 = Some(compute_hash(&r2).unwrap());

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn chain_rejects_out_of_order_timestamps() {
    let r1 = hashed_receipt("mock", 10, Outcome::Complete);
    let r2 = hashed_receipt("mock", 0, Outcome::Complete); // earlier

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::BrokenLink { index: 1 }));
}

#[test]
fn chain_empty_verify_returns_error() {
    let chain = ReceiptChain::new();
    let err = chain.verify().unwrap_err();
    assert!(matches!(err, ChainError::EmptyChain));
}

#[test]
fn chain_with_mixed_backends() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("backend-a", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt("backend-b", 10, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt("backend-c", 20, Outcome::Failed))
        .unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_with_mixed_outcomes() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt("mock", 10, Outcome::Failed))
        .unwrap();
    chain
        .push(hashed_receipt("mock", 20, Outcome::Partial))
        .unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_tamper_middle_receipt_detected_on_verify() {
    let r1 = hashed_receipt("mock", 0, Outcome::Complete);
    let mut r2 = hashed_receipt("mock", 10, Outcome::Complete);
    let r3 = hashed_receipt("mock", 20, Outcome::Complete);

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();

    // Tamper with r2's outcome after hashing
    r2.outcome = Outcome::Failed;
    // Keep the old hash (now invalid)
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::HashMismatch { index: 1 }));

    // r3 should still push fine
    chain.push(r3).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_work_order_ids_form_logical_sequence() {
    let wo_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

    let mut chain = ReceiptChain::new();
    for (i, wo_id) in wo_ids.iter().enumerate() {
        let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .to_utc()
            + Duration::seconds(i as i64 * 10);
        let finish = start + Duration::milliseconds(100);
        let r = ReceiptBuilder::new("mock")
            .work_order_id(*wo_id)
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(finish)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }

    assert_eq!(chain.len(), 3);
    let chain_wo_ids: Vec<Uuid> = chain.iter().map(|r| r.meta.work_order_id).collect();
    assert_eq!(chain_wo_ids, wo_ids);
}

// ===========================================================================
// 5. Parallel work orders produce independent valid receipts
// ===========================================================================

#[test]
fn parallel_receipts_have_unique_run_ids() {
    let receipts: Vec<Receipt> = (0..10)
        .map(|i| hashed_receipt("mock", i, Outcome::Complete))
        .collect();

    let ids: std::collections::HashSet<Uuid> = receipts.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 10);
}

#[test]
fn parallel_receipts_all_verify_independently() {
    let receipts: Vec<Receipt> = (0..10)
        .map(|i| hashed_receipt("mock", i, Outcome::Complete))
        .collect();

    for r in &receipts {
        assert!(verify_hash(r));
    }
}

#[test]
fn parallel_receipts_have_unique_hashes() {
    let receipts: Vec<Receipt> = (0..10)
        .map(|i| hashed_receipt("mock", i, Outcome::Complete))
        .collect();

    let hashes: std::collections::HashSet<String> = receipts
        .iter()
        .map(|r| r.receipt_sha256.clone().unwrap())
        .collect();
    assert_eq!(hashes.len(), 10);
}

#[test]
fn parallel_receipts_can_form_valid_chain() {
    let mut chain = ReceiptChain::new();
    for i in 0..10 {
        chain
            .push(hashed_receipt("mock", i * 5, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 10);
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 6. Receipt timestamps are monotonic
// ===========================================================================

#[test]
fn receipt_started_before_or_at_finished() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.meta.started_at <= r.meta.finished_at);
}

#[test]
fn receipt_duration_matches_timestamp_diff() {
    let start = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = start + Duration::milliseconds(500);
    let r = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(finish)
        .build();
    assert_eq!(r.meta.duration_ms, 500);
}

#[test]
fn receipt_duration_zero_when_start_equals_finish() {
    let ts = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let r = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn chain_enforces_monotonic_started_at() {
    let r1 = hashed_receipt("mock", 100, Outcome::Complete);
    let r2 = hashed_receipt("mock", 50, Outcome::Complete); // earlier

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    assert!(chain.push(r2).is_err());
}

#[test]
fn chain_allows_equal_start_times() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = base + Duration::milliseconds(100);

    let r1 = ReceiptBuilder::new("mock")
        .started_at(base)
        .finished_at(finish)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .started_at(base)
        .finished_at(finish)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn many_receipts_in_order_chain_verifies() {
    let mut chain = ReceiptChain::new();
    for i in 0..50 {
        chain
            .push(hashed_receipt("mock", i, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 50);
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 7. Receipt status values (completed, failed, partial)
// ===========================================================================

#[test]
fn receipt_outcome_complete() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_outcome_failed() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_outcome_partial() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Partial);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_outcome_serde_roundtrip_complete() {
    let outcome: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(outcome, Outcome::Complete);
    let s = serde_json::to_string(&outcome).unwrap();
    assert_eq!(s, r#""complete""#);
}

#[test]
fn receipt_outcome_serde_roundtrip_failed() {
    let outcome: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(outcome, Outcome::Failed);
}

#[test]
fn receipt_outcome_serde_roundtrip_partial() {
    let outcome: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(outcome, Outcome::Partial);
}

#[test]
fn different_outcomes_produce_different_hashes() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = base + Duration::milliseconds(42);

    let r_complete = {
        let mut r = deterministic_receipt(Uuid::nil(), Uuid::nil(), base, finish);
        r.outcome = Outcome::Complete;
        receipt_hash(&r).unwrap()
    };
    let r_failed = {
        let mut r = deterministic_receipt(Uuid::nil(), Uuid::nil(), base, finish);
        r.outcome = Outcome::Failed;
        receipt_hash(&r).unwrap()
    };
    let r_partial = {
        let mut r = deterministic_receipt(Uuid::nil(), Uuid::nil(), base, finish);
        r.outcome = Outcome::Partial;
        receipt_hash(&r).unwrap()
    };

    assert_ne!(r_complete, r_failed);
    assert_ne!(r_complete, r_partial);
    assert_ne!(r_failed, r_partial);
}

// ===========================================================================
// 8. Receipt with tool use events
// ===========================================================================

#[test]
fn receipt_with_tool_call_event_hashes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_with_tool_result_event_hashes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_tool_call_and_result_pair() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_2".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_2".into()),
            output: json!({"stdout": "file.txt\n"}),
            is_error: false,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 2);
}

#[test]
fn receipt_tool_error_result() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_3".into()),
            output: json!({"error": "command not found"}),
            is_error: true,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_nested_tool_calls() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "multi_tool".into(),
            tool_use_id: Some("parent_1".into()),
            parent_tool_use_id: None,
            input: json!({"action": "orchestrate"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("child_1".into()),
            parent_tool_use_id: Some("parent_1".into()),
            input: json!({"path": "foo.rs"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("child_1".into()),
            output: json!("contents"),
            is_error: false,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn receipt_with_file_changed_event() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added new function".into(),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_command_executed_event() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

// ===========================================================================
// 9. Receipt with streaming events accumulation
// ===========================================================================

#[test]
fn receipt_with_delta_events() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "world".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "Hello world".into(),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn receipt_streaming_deltas_affect_hash() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let ts = base;

    let r1 = {
        let mut r =
            deterministic_receipt(Uuid::nil(), Uuid::nil(), base, base + Duration::seconds(1));
        r.trace.push(make_event_at(
            AgentEventKind::AssistantDelta { text: "Hi".into() },
            ts,
        ));
        r
    };

    let r2 = {
        let mut r =
            deterministic_receipt(Uuid::nil(), Uuid::nil(), base, base + Duration::seconds(1));
        r.trace.push(make_event_at(
            AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ts,
        ));
        r
    };

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_with_full_event_lifecycle() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "I will ".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "help.".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "I will help.".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.txt"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("contents"),
            is_error: false,
        }))
        .add_trace_event(make_event(AgentEventKind::FileChanged {
            path: "a.txt".into(),
            summary: "updated".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 8);
}

#[test]
fn receipt_many_delta_events_accumulate() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..100 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: format!("token_{i} "),
        }));
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 100);
}

#[test]
fn receipt_warning_and_error_events() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(make_event(AgentEventKind::Warning {
            message: "rate limit approaching".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::Error {
            message: "rate limit exceeded".into(),
            error_code: None,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 2);
}

// ===========================================================================
// 10. Receipt schema validation against JSON schema
// ===========================================================================

#[test]
fn receipt_validates_against_json_schema() {
    let schema_str = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let instance = serde_json::to_value(&r).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let result = validator.validate(&instance);
    assert!(result.is_ok(), "Receipt should validate against schema");
}

#[test]
fn receipt_with_trace_validates_against_schema() {
    let schema_str = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .unwrap();

    let instance = serde_json::to_value(&r).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    assert!(validator.validate(&instance).is_ok());
}

#[test]
fn receipt_with_all_outcomes_validates_schema() {
    let schema_str = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();

    for outcome in [Outcome::Complete, Outcome::Failed, Outcome::Partial] {
        let r = ReceiptBuilder::new("mock")
            .outcome(outcome)
            .with_hash()
            .unwrap();
        let instance = serde_json::to_value(&r).unwrap();
        assert!(validator.validate(&instance).is_ok());
    }
}

#[test]
fn receipt_without_hash_validates_schema() {
    let schema_str = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let instance = serde_json::to_value(&r).unwrap();
    assert!(validator.validate(&instance).is_ok());
}

#[test]
fn receipt_with_artifacts_validates_schema() {
    let schema_str = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .with_hash()
        .unwrap();
    let instance = serde_json::to_value(&r).unwrap();
    assert!(validator.validate(&instance).is_ok());
}

// ===========================================================================
// 11. receipt_hash() null-field behavior (sha256 is null before hashing)
// ===========================================================================

#[test]
fn hash_ignores_existing_receipt_sha256_value() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    let hash_when_none = receipt_hash(&r).unwrap();

    r.receipt_sha256 = Some("some_old_value".into());
    let hash_when_some = receipt_hash(&r).unwrap();

    assert_eq!(hash_when_none, hash_when_some);
}

#[test]
fn hash_ignores_its_own_stored_value() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    // Hash should verify even though receipt_sha256 has a value
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[test]
fn canonicalize_sets_sha256_to_null() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some("should_be_null_in_canonical".into());

    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["receipt_sha256"].is_null());
}

#[test]
fn with_hash_then_verify_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn double_with_hash_is_idempotent() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn verify_hash_rejects_wrong_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_accepts_none() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(verify_hash(&r));
}

// ===========================================================================
// 12. Very large receipts hash correctly
// ===========================================================================

#[test]
fn large_receipt_with_many_trace_events() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..1000 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: format!("token_{i} "),
        }));
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 1000);
}

#[test]
fn large_receipt_with_big_tool_output() {
    let big_output = "x".repeat(100_000);
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_big".into()),
            output: json!(big_output),
            is_error: false,
        }))
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn large_receipt_with_many_artifacts() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "log".into(),
            path: format!("logs/step_{i}.log"),
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.artifacts.len(), 500);
}

#[test]
fn large_receipt_hash_is_still_64_chars() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: format!("Message number {i} with some padding text to make it bigger"),
        }));
    }
    let r = builder.with_hash().unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn large_receipt_with_deep_json_usage_raw() {
    let nested = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "data": (0..100).collect::<Vec<i32>>()
                    }
                }
            }
        }
    });

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(nested)
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn large_chain_of_100_receipts() {
    let mut chain = ReceiptChain::new();
    for i in 0..100 {
        chain
            .push(hashed_receipt("mock", i, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 13. Unicode content in receipts
// ===========================================================================

#[test]
fn receipt_with_unicode_task_text() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "你好世界 🌍 مرحبا".into(),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_emoji_backend_id() {
    let r = ReceiptBuilder::new("🤖-backend")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.backend.id, "🤖-backend");
}

#[test]
fn receipt_with_japanese_content() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "日本語テスト: こんにちは".into(),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_rtl_arabic_content() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "مرحبا بالعالم".into(),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_mixed_unicode_scripts() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "Latin Кириллица 日本語 한국어 العربية 🎉🎊".into(),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_unicode_in_tool_input() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"path": "文件.txt", "content": "内容 содержание"}),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_null_bytes_in_content() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "before\0after".into(),
        }))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_unicode_hash_determinism() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let ts = base;
    let text = "你好 🌍 مرحبا Кириллица";

    let make = || {
        let mut r =
            deterministic_receipt(Uuid::nil(), Uuid::nil(), base, base + Duration::seconds(1));
        r.trace.push(make_event_at(
            AgentEventKind::AssistantMessage { text: text.into() },
            ts,
        ));
        receipt_hash(&r).unwrap()
    };

    assert_eq!(make(), make());
}

// ===========================================================================
// 14. Empty/minimal receipts
// ===========================================================================

#[test]
fn minimal_receipt_builds() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete); // default
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn minimal_receipt_hashes() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn minimal_receipt_serializes_to_json() {
    let r = ReceiptBuilder::new("mock").build();
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn minimal_receipt_round_trips_through_json() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.meta.run_id, r2.meta.run_id);
}

#[test]
fn empty_backend_id_receipt_hashes() {
    let r = ReceiptBuilder::new("").with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.backend.id, "");
}

#[test]
fn receipt_with_empty_trace_and_artifacts() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    let r = r.with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_default_usage_is_empty() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
}

#[test]
fn receipt_default_mode_is_mapped() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn receipt_default_verification_is_empty() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
    assert!(!r.verification.harness_ok);
}

// ===========================================================================
// Additional: receipt serde roundtrip integrity
// ===========================================================================

#[test]
fn receipt_json_roundtrip_preserves_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .with_hash()
        .unwrap();

    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn receipt_json_roundtrip_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .capabilities(caps)
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_json_roundtrip_with_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: None,
        estimated_cost_usd: Some(0.01),
    };

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(usage)
        .usage_raw(json!({"model": "gpt-4", "total_tokens": 300}))
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

// ===========================================================================
// Additional: sha256_hex utility
// ===========================================================================

#[test]
fn sha256_hex_empty_input() {
    let hex = sha256_hex(b"");
    assert_eq!(hex.len(), 64);
    // Known SHA-256 of empty string
    assert_eq!(
        hex,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_known_value() {
    let hex = sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
    assert_eq!(
        hex,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

// ===========================================================================
// Additional: chain iteration and properties
// ===========================================================================

#[test]
fn chain_is_empty_when_new() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
}

#[test]
fn chain_iteration_order_matches_push_order() {
    let mut chain = ReceiptChain::new();
    let mut expected_ids = Vec::new();
    for i in 0..5 {
        let r = hashed_receipt("mock", i * 10, Outcome::Complete);
        expected_ids.push(r.meta.run_id);
        chain.push(r).unwrap();
    }

    let actual_ids: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(actual_ids, expected_ids);
}

#[test]
fn chain_into_iter_works() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt("mock", 10, Outcome::Complete))
        .unwrap();

    let count = (&chain).into_iter().count();
    assert_eq!(count, 2);
}

// ===========================================================================
// Additional: receipt with passthrough mode
// ===========================================================================

#[test]
fn receipt_passthrough_mode_hashes() {
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_mode_affects_hash() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let finish = base + Duration::milliseconds(42);

    let r_mapped = {
        let mut r = deterministic_receipt(Uuid::nil(), Uuid::nil(), base, finish);
        r.mode = ExecutionMode::Mapped;
        receipt_hash(&r).unwrap()
    };
    let r_passthrough = {
        let mut r = deterministic_receipt(Uuid::nil(), Uuid::nil(), base, finish);
        r.mode = ExecutionMode::Passthrough;
        receipt_hash(&r).unwrap()
    };

    assert_ne!(r_mapped, r_passthrough);
}

// ===========================================================================
// Additional: receipt with ext field on events
// ===========================================================================

#[test]
fn receipt_with_ext_field_on_event() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
}

#[test]
fn receipt_ext_field_affects_hash() {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let ts = base;

    let r1 = {
        let mut r =
            deterministic_receipt(Uuid::nil(), Uuid::nil(), base, base + Duration::seconds(1));
        r.trace.push(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        });
        receipt_hash(&r).unwrap()
    };

    let r2 = {
        let mut ext = BTreeMap::new();
        ext.insert("key".to_string(), json!("value"));
        let mut r =
            deterministic_receipt(Uuid::nil(), Uuid::nil(), base, base + Duration::seconds(1));
        r.trace.push(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        });
        receipt_hash(&r).unwrap()
    };

    assert_ne!(r1, r2);
}

// ===========================================================================
// Additional: receipt with verification report
// ===========================================================================

#[test]
fn receipt_with_full_verification_report() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .verification(VerificationReport {
            git_diff: Some("diff --git a/file.rs b/file.rs\n+new line".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_some());
}

// ===========================================================================
// Additional: receipt chain error display
// ===========================================================================

#[test]
fn chain_error_display_hash_mismatch() {
    let err = ChainError::HashMismatch { index: 3 };
    assert_eq!(err.to_string(), "hash mismatch at chain index 3");
}

#[test]
fn chain_error_display_broken_link() {
    let err = ChainError::BrokenLink { index: 2 };
    assert_eq!(err.to_string(), "broken link at chain index 2");
}

#[test]
fn chain_error_display_empty_chain() {
    let err = ChainError::EmptyChain;
    assert_eq!(err.to_string(), "chain is empty");
}

#[test]
fn chain_error_display_duplicate_id() {
    let id = Uuid::nil();
    let err = ChainError::DuplicateId { id };
    assert!(err.to_string().contains("duplicate receipt id"));
}

// ===========================================================================
// Runtime integration: MockBackend produces valid receipts
// ===========================================================================

#[tokio::test]
async fn mock_backend_produces_valid_receipt() {
    use abp_integrations::{Backend, MockBackend};

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test task").build();
    let run_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();

    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn mock_backend_receipt_has_correct_run_id() {
    use abp_integrations::{Backend, MockBackend};

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test").build();
    let run_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn mock_backend_receipt_references_work_order() {
    use abp_integrations::{Backend, MockBackend};

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test").build();
    let wo_id = wo.id;
    let run_id = Uuid::new_v4();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn mock_backend_streams_events() {
    use abp_integrations::{Backend, MockBackend};

    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test").build();
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);

    let _receipt = backend.run(run_id, wo, tx).await.unwrap();

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    // MockBackend emits RunStarted, 2x AssistantMessage, RunCompleted
    assert!(events.len() >= 3);
}
