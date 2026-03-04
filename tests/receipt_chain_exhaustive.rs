#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore, StoreError};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    ChainBuilder, ChainError, ChainGap, ChainSummary, FieldDiff, Outcome, Receipt, ReceiptBuilder,
    ReceiptChain, ReceiptDiff, ReceiptValidator, TamperEvidence, TamperKind, ValidationError,
    canonicalize, compute_hash, diff_receipts, verify_hash,
};
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

fn make_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_hashed_receipt(backend: &str, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .with_hash()
        .unwrap()
}

fn make_receipt_at(backend: &str, outcome: Outcome, offset_ms: i64) -> Receipt {
    let base = Utc::now() + ChronoDuration::milliseconds(offset_ms);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(base)
        .finished_at(base)
        .build()
}

fn make_hashed_receipt_at(backend: &str, outcome: Outcome, offset_ms: i64) -> Receipt {
    let base = Utc::now() + ChronoDuration::milliseconds(offset_ms);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(base)
        .finished_at(base)
        .with_hash()
        .unwrap()
}

fn make_receipt_with_duration(backend: &str, outcome: Outcome, dur_ms: i64) -> Receipt {
    let start = Utc::now();
    let end = start + ChronoDuration::milliseconds(dur_ms);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(end)
        .with_hash()
        .unwrap()
}

fn make_receipt_with_tokens(backend: &str, input: u64, output: u64) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .usage_tokens(input, output)
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// Section 1: Receipt Creation and Hashing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_creation_basic() {
    let r = make_receipt("mock", Outcome::Complete);
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn receipt_creation_failed() {
    let r = make_receipt("mock", Outcome::Failed);
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_creation_partial() {
    let r = make_receipt("mock", Outcome::Partial);
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_with_hash_has_sha256() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_is_hex_string() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let h = r.receipt_sha256.unwrap();
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_deterministic() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_length_64() {
    let r = make_receipt("mock", Outcome::Complete);
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = make_receipt("mock", Outcome::Complete);
    let r2 = {
        let mut r = r1.clone();
        r.outcome = Outcome::Failed;
        r
    };
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_changes_with_backend() {
    let r1 = make_receipt("alpha", Outcome::Complete);
    let r2 = make_receipt("beta", Outcome::Complete);
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn receipt_verify_hash_valid() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_verify_hash_no_hash_is_valid() {
    let r = make_receipt("mock", Outcome::Complete);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_verify_hash_tampered() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".repeat(8));
    assert!(!verify_hash(&r));
}

#[test]
fn receipt_verify_hash_after_field_change() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: Canonical JSON (self-referential prevention)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn canonicalize_sets_receipt_sha256_to_null() {
    let mut r = make_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("something".into());
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn canonicalize_deterministic() {
    let r = make_receipt("mock", Outcome::Complete);
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonicalize_independent_of_stored_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let j1 = canonicalize(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("abc123".into());
    let j2 = canonicalize(&r2).unwrap();

    assert_eq!(j1, j2);
}

#[test]
fn hash_independent_of_stored_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("totally_different".into());
    let h2 = compute_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn canonicalize_is_valid_json() {
    let r = make_receipt("mock", Outcome::Complete);
    let json = canonicalize(&r).unwrap();
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

#[test]
fn canonicalize_with_hash_set_yields_null_in_output() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn hash_recompute_matches_after_with_hash() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let stored = r.receipt_sha256.clone().unwrap();
    let recomputed = compute_hash(&r).unwrap();
    assert_eq!(stored, recomputed);
}

#[test]
fn canonical_json_contains_backend_id() {
    let r = make_receipt("test-backend", Outcome::Complete);
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("test-backend"));
}

#[test]
fn canonical_json_contains_outcome() {
    let r = make_receipt("mock", Outcome::Failed);
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Receipt Chain Building
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_push_single() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_push_multiple_sequential() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let r = make_hashed_receipt_at("mock", Outcome::Complete, i * 1000);
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 5);
}

#[test]
fn chain_verify_single() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_empty_fails() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_verify_multiple() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_reject_duplicate_id() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let r2 = r.clone();
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    let err = chain.push(r2).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn chain_reject_out_of_order() {
    let later = make_hashed_receipt_at("mock", Outcome::Complete, 5000);
    let earlier = make_hashed_receipt_at("mock", Outcome::Complete, -5000);

    let mut chain = ReceiptChain::new();
    chain.push(later).unwrap();
    let err = chain.push(earlier).unwrap_err();
    assert!(matches!(err, ChainError::BrokenLink { .. }));
}

#[test]
fn chain_reject_hash_mismatch() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("bad_hash".to_string());
    let mut chain = ReceiptChain::new();
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_latest() {
    let mut chain = ReceiptChain::new();
    let r1 = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    let r2 = make_hashed_receipt_at("mock", Outcome::Failed, 1000);
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[test]
fn chain_get_by_index() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.get(0).unwrap().meta.run_id, id);
    assert!(chain.get(1).is_none());
}

#[test]
fn chain_sequence_at() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert_eq!(chain.sequence_at(0), Some(0));
    assert_eq!(chain.sequence_at(1), Some(1));
    assert_eq!(chain.sequence_at(2), Some(2));
    assert_eq!(chain.sequence_at(3), None);
}

#[test]
fn chain_parent_hash_at() {
    let mut chain = ReceiptChain::new();
    let r1 = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    let r1_hash = r1.receipt_sha256.clone();
    chain.push(r1).unwrap();

    let r2 = make_hashed_receipt_at("mock", Outcome::Complete, 1000);
    chain.push(r2).unwrap();

    assert!(chain.parent_hash_at(0).is_none());
    assert_eq!(chain.parent_hash_at(1), r1_hash.as_deref());
}

#[test]
fn chain_iterator() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert_eq!(chain.iter().count(), 3);
}

#[test]
fn chain_as_slice() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert_eq!(chain.as_slice().len(), 1);
}

#[test]
fn chain_into_iter_ref() {
    let mut chain = ReceiptChain::new();
    for i in 0..2 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    let mut count = 0;
    for _ in &chain {
        count += 1;
    }
    assert_eq!(count, 2);
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Chain Integrity and Tampering Detection
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_verify_chain_ok() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_verify_chain_empty_fails() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify_chain(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_detect_tampering_clean() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn chain_detect_tampering_hash_mismatch() {
    let chain = ChainBuilder::new()
        .skip_validation()
        .append({
            let mut r = make_hashed_receipt("mock", Outcome::Complete);
            r.receipt_sha256 = Some("tampered_hash_value".to_string());
            r
        })
        .unwrap()
        .build();

    let evidence = chain.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(matches!(evidence[0].kind, TamperKind::HashMismatch { .. }));
}

#[test]
fn chain_detect_tampering_returns_all_evidence() {
    let chain = ChainBuilder::new()
        .skip_validation()
        .append({
            let mut r = make_hashed_receipt("mock", Outcome::Complete);
            r.receipt_sha256 = Some("bad1".to_string());
            r
        })
        .unwrap()
        .append({
            let mut r = make_hashed_receipt("mock", Outcome::Failed);
            r.receipt_sha256 = Some("bad2".to_string());
            r
        })
        .unwrap()
        .build();

    let evidence = chain.detect_tampering();
    assert!(evidence.len() >= 2);
}

#[test]
fn chain_find_gaps_no_gaps() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn chain_find_gaps_with_gap() {
    let r1 = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    let r2 = make_hashed_receipt_at("mock", Outcome::Complete, 1000);

    let chain = ChainBuilder::new()
        .append_with_sequence(r1, 0)
        .unwrap()
        .append_with_sequence(r2, 5)
        .unwrap()
        .build();

    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

// ═══════════════════════════════════════════════════════════════════
// Section 5: Chain Summary
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_summary_empty() {
    let chain = ReceiptChain::new();
    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 0);
    assert_eq!(summary.complete_count, 0);
    assert_eq!(summary.failed_count, 0);
    assert_eq!(summary.partial_count, 0);
}

#[test]
fn chain_summary_counts_outcomes() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt_at("mock", Outcome::Complete, 0))
        .unwrap();
    chain
        .push(make_hashed_receipt_at("mock", Outcome::Failed, 1000))
        .unwrap();
    chain
        .push(make_hashed_receipt_at("mock", Outcome::Partial, 2000))
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 3);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.partial_count, 1);
}

#[test]
fn chain_summary_tracks_backends() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt_at("alpha", Outcome::Complete, 0))
        .unwrap();
    chain
        .push(make_hashed_receipt_at("beta", Outcome::Complete, 1000))
        .unwrap();
    chain
        .push(make_hashed_receipt_at("alpha", Outcome::Complete, 2000))
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.backends.len(), 2);
    assert!(summary.backends.contains(&"alpha".to_string()));
    assert!(summary.backends.contains(&"beta".to_string()));
}

#[test]
fn chain_summary_all_hashes_valid() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    assert!(chain.chain_summary().all_hashes_valid);
}

#[test]
fn chain_summary_all_hashes_valid_no_hash() {
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(make_receipt("mock", Outcome::Complete))
        .unwrap()
        .build();
    assert!(chain.chain_summary().all_hashes_valid);
}

#[test]
fn chain_summary_invalid_hash() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("tampered".into());
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert!(!chain.chain_summary().all_hashes_valid);
}

#[test]
fn chain_summary_gap_count() {
    let r1 = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    let r2 = make_hashed_receipt_at("mock", Outcome::Complete, 1000);

    let chain = ChainBuilder::new()
        .append_with_sequence(r1, 0)
        .unwrap()
        .append_with_sequence(r2, 10)
        .unwrap()
        .build();

    assert_eq!(chain.chain_summary().gap_count, 1);
}

#[test]
fn chain_summary_total_duration() {
    let start = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start + ChronoDuration::milliseconds(100))
        .with_hash()
        .unwrap();

    let start2 = start + ChronoDuration::milliseconds(1000);
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start2)
        .finished_at(start2 + ChronoDuration::milliseconds(200))
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    assert_eq!(chain.chain_summary().total_duration_ms, 300);
}

#[test]
fn chain_summary_token_totals() {
    let start = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .usage_tokens(100, 50)
        .with_hash()
        .unwrap();

    let start2 = start + ChronoDuration::milliseconds(1000);
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start2)
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
fn chain_summary_timestamps() {
    let start = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(start + ChronoDuration::milliseconds(100))
        .with_hash()
        .unwrap();

    let start2 = start + ChronoDuration::milliseconds(1000);
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(start2)
        .finished_at(start2 + ChronoDuration::milliseconds(200))
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.first_started_at, Some(start));
    assert_eq!(
        summary.last_finished_at,
        Some(start2 + ChronoDuration::milliseconds(200))
    );
}

// ═══════════════════════════════════════════════════════════════════
// Section 6: ChainBuilder
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_builder_basic() {
    let chain = ChainBuilder::new()
        .append(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_multiple() {
    let chain = ChainBuilder::new()
        .append(make_hashed_receipt_at("mock", Outcome::Complete, 0))
        .unwrap()
        .append(make_hashed_receipt_at("mock", Outcome::Failed, 1000))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_builder_skip_validation() {
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(make_receipt("mock", Outcome::Complete))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_append_with_sequence() {
    let r1 = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    let r2 = make_hashed_receipt_at("mock", Outcome::Complete, 1000);

    let chain = ChainBuilder::new()
        .append_with_sequence(r1, 0)
        .unwrap()
        .append_with_sequence(r2, 42)
        .unwrap()
        .build();

    assert_eq!(chain.len(), 2);
    assert_eq!(chain.sequence_at(0), Some(0));
    assert_eq!(chain.sequence_at(1), Some(42));
}

#[test]
fn chain_builder_rejects_duplicate() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let r2 = r.clone();
    let result = ChainBuilder::new().append(r).unwrap().append(r2);
    assert!(result.is_err());
}

#[test]
fn chain_builder_default() {
    let builder = ChainBuilder::default();
    let chain = builder.build();
    assert!(chain.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Section 7: Receipt Diff
// ═══════════════════════════════════════════════════════════════════

#[test]
fn diff_identical_receipts() {
    let r = make_receipt("mock", Outcome::Complete);
    let diff = diff_receipts(&r, &r);
    assert!(diff.is_empty());
    assert_eq!(diff.len(), 0);
}

#[test]
fn diff_cloned_receipts() {
    let r = make_receipt("mock", Outcome::Complete);
    let r2 = r.clone();
    let diff = diff_receipts(&r, &r2);
    assert!(diff.is_empty());
}

#[test]
fn diff_different_outcome() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let diff = diff_receipts(&a, &b);
    assert!(!diff.is_empty());
    assert!(diff.changes.iter().any(|d| d.field == "outcome"));
}

#[test]
fn diff_different_backend() {
    let a = make_receipt("alpha", Outcome::Complete);
    let b = make_receipt("beta", Outcome::Complete);
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "backend.id"));
}

#[test]
fn diff_different_run_id() {
    let a = make_receipt("mock", Outcome::Complete);
    let b = make_receipt("mock", Outcome::Complete);
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "meta.run_id"));
}

#[test]
fn diff_different_backend_version() {
    let a = ReceiptBuilder::new("mock").backend_version("1.0").build();
    let b = ReceiptBuilder::new("mock").backend_version("2.0").build();
    let diff = diff_receipts(&a, &b);
    assert!(
        diff.changes
            .iter()
            .any(|d| d.field == "backend.backend_version")
    );
}

#[test]
fn diff_different_adapter_version() {
    let a = ReceiptBuilder::new("mock").adapter_version("1.0").build();
    let b = ReceiptBuilder::new("mock").adapter_version("2.0").build();
    let diff = diff_receipts(&a, &b);
    assert!(
        diff.changes
            .iter()
            .any(|d| d.field == "backend.adapter_version")
    );
}

#[test]
fn diff_different_duration() {
    let start = Utc::now();
    let a = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(start + ChronoDuration::milliseconds(100))
        .build();
    let b = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(start + ChronoDuration::milliseconds(200))
        .build();
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "meta.duration_ms"));
}

#[test]
fn diff_different_trace_len() {
    use abp_receipt::{AgentEvent, AgentEventKind};
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    });
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "trace.len"));
}

#[test]
fn diff_field_old_and_new_values() {
    let a = make_receipt("mock", Outcome::Complete);
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let diff = diff_receipts(&a, &b);
    let outcome_diff = diff.changes.iter().find(|d| d.field == "outcome").unwrap();
    assert!(outcome_diff.old.contains("Complete"));
    assert!(outcome_diff.new.contains("Failed"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 8: Receipt Store (InMemoryReceiptStore)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn store_new_is_empty() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn store_add_and_get() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let id = r.meta.run_id;
    store.store(r).unwrap();
    assert_eq!(store.len(), 1);
    assert!(store.get(id).unwrap().is_some());
}

#[test]
fn store_get_nonexistent() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get(Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn store_add_multiple() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..10 {
        store
            .store(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert_eq!(store.len(), 10);
}

#[test]
fn store_reject_duplicate() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let r2 = r.clone();
    store.store(r).unwrap();
    let err = store.store(r2).unwrap_err();
    assert!(matches!(err, StoreError::DuplicateId(_)));
}

#[test]
fn store_list_all() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..5 {
        store
            .store(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 5);
}

#[test]
fn store_list_empty() {
    let store = InMemoryReceiptStore::new();
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert!(all.is_empty());
}

#[test]
fn store_returns_correct_id() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let expected_id = r.meta.run_id;
    let returned_id = store.store(r).unwrap();
    assert_eq!(returned_id, expected_id);
}

#[test]
fn store_get_returns_correct_receipt() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_hashed_receipt("test-backend", Outcome::Failed);
    let id = r.meta.run_id;
    store.store(r).unwrap();
    let retrieved = store.get(id).unwrap().unwrap();
    assert_eq!(retrieved.backend.id, "test-backend");
    assert_eq!(retrieved.outcome, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════
// Section 9: Receipt Filtering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("alpha", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("beta", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("alpha", Outcome::Failed))
        .unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|s| s.backend_id == "alpha"));
}

#[test]
fn filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("mock", Outcome::Failed))
        .unwrap();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn filter_by_outcome_failed() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("mock", Outcome::Failed))
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_by_outcome_partial() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Partial))
        .unwrap();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Partial),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_by_timestamp_after() {
    let mut store = InMemoryReceiptStore::new();
    let t1 = Utc::now() - ChronoDuration::hours(2);
    let t2 = Utc::now();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t1)
        .finished_at(t1)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t2)
        .finished_at(t2)
        .with_hash()
        .unwrap();
    store.store(r1).unwrap();
    store.store(r2).unwrap();

    let filter = ReceiptFilter {
        after: Some(Utc::now() - ChronoDuration::hours(1)),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_by_timestamp_before() {
    let mut store = InMemoryReceiptStore::new();
    let t1 = Utc::now() - ChronoDuration::hours(2);
    let t2 = Utc::now();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t1)
        .finished_at(t1)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t2)
        .finished_at(t2)
        .with_hash()
        .unwrap();
    store.store(r1).unwrap();
    store.store(r2).unwrap();

    let filter = ReceiptFilter {
        before: Some(Utc::now() - ChronoDuration::hours(1)),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_by_timestamp_range() {
    let mut store = InMemoryReceiptStore::new();
    let t1 = Utc::now() - ChronoDuration::hours(3);
    let t2 = Utc::now() - ChronoDuration::hours(1);
    let t3 = Utc::now();

    for t in [t1, t2, t3] {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(t)
            .finished_at(t)
            .with_hash()
            .unwrap();
        store.store(r).unwrap();
    }

    let filter = ReceiptFilter {
        after: Some(Utc::now() - ChronoDuration::hours(2)),
        before: Some(Utc::now() - ChronoDuration::minutes(30)),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_combined_backend_and_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("alpha", Outcome::Complete))
        .unwrap();
    store
        .store(make_hashed_receipt("alpha", Outcome::Failed))
        .unwrap();
    store
        .store(make_hashed_receipt("beta", Outcome::Complete))
        .unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        outcome: Some(Outcome::Complete),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn filter_no_match() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();

    let filter = ReceiptFilter {
        backend_id: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert!(results.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Section 10: Receipt Ordering (by timestamp)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_enforces_chronological_order() {
    let later = make_hashed_receipt_at("mock", Outcome::Complete, 5000);
    let earlier = make_hashed_receipt_at("mock", Outcome::Complete, -5000);

    let mut chain = ReceiptChain::new();
    chain.push(later).unwrap();
    assert!(chain.push(earlier).is_err());
}

#[test]
fn chain_allows_same_timestamp() {
    let now = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(now)
        .finished_at(now)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(now)
        .finished_at(now)
        .with_hash()
        .unwrap();

    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn store_list_summary_has_timestamps() {
    let mut store = InMemoryReceiptStore::new();
    let now = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(now)
        .finished_at(now + ChronoDuration::milliseconds(100))
        .with_hash()
        .unwrap();
    store.store(r).unwrap();
    let summaries = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(summaries[0].started_at, now);
    assert_eq!(
        summaries[0].finished_at,
        now + ChronoDuration::milliseconds(100)
    );
}

// ═══════════════════════════════════════════════════════════════════
// Section 11: Concurrent Receipt Store Access
// ═══════════════════════════════════════════════════════════════════

#[test]
fn concurrent_store_access() {
    let store = Arc::new(Mutex::new(InMemoryReceiptStore::new()));
    let mut handles = vec![];

    for _ in 0..10 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            let r = make_hashed_receipt("mock", Outcome::Complete);
            let mut s = store.lock().unwrap();
            s.store(r).unwrap();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let s = store.lock().unwrap();
    assert_eq!(s.len(), 10);
}

#[test]
fn concurrent_store_and_read() {
    let store = Arc::new(Mutex::new(InMemoryReceiptStore::new()));
    let mut handles = vec![];

    // Writers
    for _ in 0..5 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            let r = make_hashed_receipt("mock", Outcome::Complete);
            let mut s = store.lock().unwrap();
            s.store(r).unwrap();
        }));
    }

    // Readers
    for _ in 0..5 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            let s = store.lock().unwrap();
            let _ = s.list(&ReceiptFilter::default()).unwrap();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let s = store.lock().unwrap();
    assert_eq!(s.len(), 5);
}

#[test]
fn concurrent_store_no_duplicates() {
    let store = Arc::new(Mutex::new(InMemoryReceiptStore::new()));
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let r2 = r.clone();

    let s1 = Arc::clone(&store);
    let h1 = thread::spawn(move || {
        let mut s = s1.lock().unwrap();
        s.store(r)
    });

    let s2 = Arc::clone(&store);
    let h2 = thread::spawn(move || {
        let mut s = s2.lock().unwrap();
        s.store(r2)
    });

    let results: Vec<_> = vec![h1.join().unwrap(), h2.join().unwrap()];
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    let err_count = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(ok_count, 1);
    assert_eq!(err_count, 1);
}

// ═══════════════════════════════════════════════════════════════════
// Section 12: Receipt Verification
// ═══════════════════════════════════════════════════════════════════

#[test]
fn verify_receipt_valid() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.issues.is_empty());
}

#[test]
fn verify_receipt_no_hash() {
    let r = make_receipt("mock", Outcome::Complete);
    let result = verify_receipt(&r);
    assert!(result.hash_valid);
}

#[test]
fn verify_receipt_tampered_hash() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("tampered".into());
    let result = verify_receipt(&r);
    assert!(!result.hash_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_bad_contract_version() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.meta.contract_version = "wrong/v99".into();
    r.receipt_sha256 = Some(compute_hash(&r).unwrap());
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
}

#[test]
fn verify_receipt_timestamps_consistent() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = verify_receipt(&r);
    assert!(result.timestamps_valid);
}

#[test]
fn verify_receipt_display_verified() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("verified"));
}

#[test]
fn verify_receipt_display_failed() {
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("bad".into());
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("failed"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 13: Receipt Validator
// ═══════════════════════════════════════════════════════════════════

#[test]
fn validator_valid_receipt() {
    let v = ReceiptValidator::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_bad_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.contract_version = "wrong".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_empty_backend_id() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("").outcome(Outcome::Complete).build();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn validator_bad_hash() {
    let v = ReceiptValidator::new();
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("wrong_hash".into());
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn validator_duration_mismatch() {
    let v = ReceiptValidator::new();
    let mut r = make_receipt("mock", Outcome::Complete);
    r.meta.duration_ms = 999999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 14: Receipt Auditor
// ═══════════════════════════════════════════════════════════════════

#[test]
fn auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let receipts: Vec<_> = (0..3)
        .map(|_| make_hashed_receipt("mock", Outcome::Complete))
        .collect();
    let report = auditor.audit_batch(&receipts);
    assert!(report.is_clean());
    assert_eq!(report.total, 3);
    assert_eq!(report.valid, 3);
    assert_eq!(report.invalid, 0);
}

#[test]
fn auditor_empty_batch() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
}

#[test]
fn auditor_detects_invalid() {
    let auditor = ReceiptAuditor::new();
    let mut r = make_hashed_receipt("mock", Outcome::Complete);
    r.receipt_sha256 = Some("bad".into());
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_detects_duplicate_run_id() {
    let auditor = ReceiptAuditor::new();
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let r2 = r.clone();
    let report = auditor.audit_batch(&[r, r2]);
    assert!(!report.is_clean());
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.description.contains("duplicate run_id"))
    );
}

#[test]
fn auditor_report_display() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[make_hashed_receipt("mock", Outcome::Complete)]);
    let display = format!("{report}");
    assert!(display.contains("AuditReport"));
    assert!(display.contains("total: 1"));
}

#[test]
fn audit_issue_display_with_index_and_id() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: Some(0),
        run_id: Some("abc".into()),
        description: "test issue".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("#0"));
    assert!(display.contains("abc"));
    assert!(display.contains("test issue"));
}

#[test]
fn audit_issue_display_no_index() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: None,
        run_id: Some("abc".into()),
        description: "test".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("abc"));
}

#[test]
fn audit_issue_display_no_id() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: Some(1),
        run_id: None,
        description: "test".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("#1"));
}

#[test]
fn audit_issue_display_bare() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: None,
        run_id: None,
        description: "bare issue".into(),
    };
    let display = format!("{issue}");
    assert_eq!(display, "bare issue");
}

// ═══════════════════════════════════════════════════════════════════
// Section 15: Serde Formats
// ═══════════════════════════════════════════════════════════════════

#[test]
fn serde_json_roundtrip() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn serde_bytes_roundtrip() {
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn serde_json_pretty_formatted() {
    let r = make_receipt("mock", Outcome::Complete);
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    assert!(json.contains('\n'));
}

#[test]
fn serde_bytes_compact() {
    let r = make_receipt("mock", Outcome::Complete);
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(!s.contains('\n'));
}

// ═══════════════════════════════════════════════════════════════════
// Section 16: Chain Serialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_serialize_json() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    assert!(json.starts_with('['));
}

#[test]
fn chain_deserialize_json() {
    let mut chain = ReceiptChain::new();
    let r = make_hashed_receipt_at("mock", Outcome::Complete, 0);
    chain.push(r).unwrap();
    let r2 = make_hashed_receipt_at("mock", Outcome::Complete, 1000);
    chain.push(r2).unwrap();

    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain2.len(), 2);
}

#[test]
fn chain_serde_roundtrip() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_hashed_receipt_at("mock", Outcome::Complete, i * 1000))
            .unwrap();
    }
    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain.len(), chain2.len());
    assert!(chain2.verify().is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// Section 17: ChainError Display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn chain_error_hash_mismatch_display() {
    let e = ChainError::HashMismatch { index: 3 };
    assert_eq!(format!("{e}"), "hash mismatch at chain index 3");
}

#[test]
fn chain_error_broken_link_display() {
    let e = ChainError::BrokenLink { index: 1 };
    assert_eq!(format!("{e}"), "broken link at chain index 1");
}

#[test]
fn chain_error_empty_chain_display() {
    let e = ChainError::EmptyChain;
    assert_eq!(format!("{e}"), "chain is empty");
}

#[test]
fn chain_error_duplicate_id_display() {
    let id = Uuid::nil();
    let e = ChainError::DuplicateId { id };
    assert!(format!("{e}").contains("duplicate receipt id"));
}

#[test]
fn chain_error_parent_mismatch_display() {
    let e = ChainError::ParentMismatch { index: 2 };
    assert!(format!("{e}").contains("parent hash mismatch"));
}

#[test]
fn chain_error_sequence_gap_display() {
    let e = ChainError::SequenceGap {
        expected: 5,
        actual: 10,
    };
    let s = format!("{e}");
    assert!(s.contains("5"));
    assert!(s.contains("10"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 18: TamperEvidence and TamperKind Display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn tamper_kind_hash_mismatch_display() {
    let tk = TamperKind::HashMismatch {
        stored: "aaa".into(),
        computed: "bbb".into(),
    };
    let s = format!("{tk}");
    assert!(s.contains("aaa"));
    assert!(s.contains("bbb"));
}

#[test]
fn tamper_kind_parent_link_broken_display() {
    let tk = TamperKind::ParentLinkBroken {
        expected: Some("exp".into()),
        actual: Some("act".into()),
    };
    let s = format!("{tk}");
    assert!(s.contains("exp"));
    assert!(s.contains("act"));
}

#[test]
fn tamper_evidence_display() {
    let te = TamperEvidence {
        index: 1,
        sequence: 5,
        kind: TamperKind::HashMismatch {
            stored: "x".into(),
            computed: "y".into(),
        },
    };
    let s = format!("{te}");
    assert!(s.contains("index=1"));
    assert!(s.contains("seq=5"));
}

#[test]
fn chain_gap_display() {
    let gap = ChainGap {
        expected: 3,
        actual: 7,
        after_index: 2,
    };
    let s = format!("{gap}");
    assert!(s.contains("3"));
    assert!(s.contains("7"));
    assert!(s.contains("2"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 19: ReceiptBuilder Advanced
// ═══════════════════════════════════════════════════════════════════

#[test]
fn builder_backend_version() {
    let r = ReceiptBuilder::new("mock").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn builder_adapter_version() {
    let r = ReceiptBuilder::new("mock").adapter_version("0.1.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1.0"));
}

#[test]
fn builder_model_in_usage_raw() {
    let r = ReceiptBuilder::new("mock").model("gpt-4").build();
    assert_eq!(r.usage_raw["model"], "gpt-4");
}

#[test]
fn builder_dialect_in_usage_raw() {
    let r = ReceiptBuilder::new("mock").dialect("openai").build();
    assert_eq!(r.usage_raw["dialect"], "openai");
}

#[test]
fn builder_duration() {
    let r = ReceiptBuilder::new("mock")
        .duration(Duration::from_millis(500))
        .build();
    assert_eq!(r.meta.duration_ms, 500);
}

#[test]
fn builder_work_order_id() {
    let woid = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock").work_order_id(woid).build();
    assert_eq!(r.meta.work_order_id, woid);
}

#[test]
fn builder_run_id() {
    let rid = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock").run_id(rid).build();
    assert_eq!(r.meta.run_id, rid);
}

#[test]
fn builder_usage_tokens() {
    let r = ReceiptBuilder::new("mock").usage_tokens(100, 50).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
}

#[test]
fn builder_error_sets_failed() {
    let r = ReceiptBuilder::new("mock").error("something broke").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(!r.trace.is_empty());
}

#[test]
fn builder_mode() {
    use abp_receipt::ExecutionMode;
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn builder_contract_version() {
    use abp_receipt::CONTRACT_VERSION;
    let r = make_receipt("mock", Outcome::Complete);
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════
// Section 20: Store Error Display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn store_error_duplicate_display() {
    let id = Uuid::nil();
    let e = StoreError::DuplicateId(id);
    assert!(format!("{e}").contains("duplicate"));
}

#[test]
fn store_error_other_display() {
    let e = StoreError::Other("disk full".into());
    assert!(format!("{e}").contains("disk full"));
}

// ═══════════════════════════════════════════════════════════════════
// Section 21: Validation Error Display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display() {
    let e = ValidationError {
        field: "backend.id".into(),
        message: "must not be empty".into(),
    };
    assert_eq!(format!("{e}"), "backend.id: must not be empty");
}

// ═══════════════════════════════════════════════════════════════════
// Section 22: Receipt Summary
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_summary_from_receipt() {
    use abp_receipt::store::ReceiptSummary;
    let r = make_hashed_receipt("mock", Outcome::Complete);
    let s = ReceiptSummary::from(&r);
    assert_eq!(s.id, r.meta.run_id);
    assert_eq!(s.backend_id, "mock");
    assert_eq!(s.outcome, Outcome::Complete);
}

#[test]
fn receipt_summary_fields() {
    use abp_receipt::store::ReceiptSummary;
    let now = Utc::now();
    let r = ReceiptBuilder::new("test")
        .outcome(Outcome::Failed)
        .started_at(now)
        .finished_at(now + ChronoDuration::milliseconds(100))
        .build();
    let s = ReceiptSummary::from(&r);
    assert_eq!(s.started_at, now);
    assert_eq!(s.finished_at, now + ChronoDuration::milliseconds(100));
    assert_eq!(s.outcome, Outcome::Failed);
}

// ═══════════════════════════════════════════════════════════════════
// Section 23: Large Chain Stress Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn large_chain_100_receipts() {
    let mut chain = ReceiptChain::new();
    let base = Utc::now();
    for i in 0..100 {
        let t = base + ChronoDuration::milliseconds(i * 100);
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(t)
            .finished_at(t)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
    assert!(chain.verify_chain().is_ok());
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn large_store_100_receipts() {
    let mut store = InMemoryReceiptStore::new();
    for _ in 0..100 {
        store
            .store(make_hashed_receipt("mock", Outcome::Complete))
            .unwrap();
    }
    assert_eq!(store.len(), 100);
    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 100);
}

#[test]
fn chain_summary_large_chain() {
    let mut chain = ReceiptChain::new();
    let base = Utc::now();
    for i in 0..50 {
        let t = base + ChronoDuration::milliseconds(i * 100);
        let outcome = if i % 3 == 0 {
            Outcome::Failed
        } else if i % 3 == 1 {
            Outcome::Partial
        } else {
            Outcome::Complete
        };
        let r = ReceiptBuilder::new(if i % 2 == 0 { "alpha" } else { "beta" })
            .outcome(outcome)
            .started_at(t)
            .finished_at(t + ChronoDuration::milliseconds(10))
            .usage_tokens(100, 50)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }

    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 50);
    assert_eq!(summary.backends.len(), 2);
    assert_eq!(summary.total_input_tokens, 5000);
    assert_eq!(summary.total_output_tokens, 2500);
    assert!(summary.all_hashes_valid);
    assert_eq!(summary.gap_count, 0);
}

// ═══════════════════════════════════════════════════════════════════
// Section 24: Edge Cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_no_hash_can_be_stored() {
    let mut store = InMemoryReceiptStore::new();
    let r = make_receipt("mock", Outcome::Complete);
    store.store(r).unwrap();
    assert_eq!(store.len(), 1);
}

#[test]
fn chain_push_receipt_without_hash() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", Outcome::Complete);
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_parent_hash_none_for_unhashed_receipts() {
    let mut chain = ReceiptChain::new();
    let r1 = make_receipt("mock", Outcome::Complete);
    chain.push(r1).unwrap();
    let r2 = make_receipt_at("mock", Outcome::Complete, 1000);
    chain.push(r2).unwrap();
    // Parent hash of second receipt is None because first has no hash
    assert!(chain.parent_hash_at(1).is_none());
}

#[test]
fn diff_receipts_usage_raw_difference() {
    let a = ReceiptBuilder::new("mock")
        .usage_raw(serde_json::json!({"key": "val1"}))
        .build();
    let mut b = a.clone();
    b.usage_raw = serde_json::json!({"key": "val2"});
    let diff = diff_receipts(&a, &b);
    assert!(diff.changes.iter().any(|d| d.field == "usage_raw"));
}

#[test]
fn chain_summary_serializable() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    let summary = chain.chain_summary();
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("total_receipts"));
}

#[test]
fn chain_gap_serializable() {
    let gap = ChainGap {
        expected: 1,
        actual: 3,
        after_index: 0,
    };
    let json = serde_json::to_string(&gap).unwrap();
    assert!(json.contains("expected"));
}

#[test]
fn tamper_evidence_serializable() {
    let te = TamperEvidence {
        index: 0,
        sequence: 0,
        kind: TamperKind::HashMismatch {
            stored: "a".into(),
            computed: "b".into(),
        },
    };
    let json = serde_json::to_string(&te).unwrap();
    assert!(json.contains("HashMismatch"));
}

#[test]
fn chain_clone() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    let chain2 = chain.clone();
    assert_eq!(chain.len(), chain2.len());
}

#[test]
fn chain_default() {
    let chain = ReceiptChain::default();
    assert!(chain.is_empty());
}

#[test]
fn store_clone() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(make_hashed_receipt("mock", Outcome::Complete))
        .unwrap();
    let store2 = store.clone();
    assert_eq!(store.len(), store2.len());
}

#[test]
fn builder_backend_alias() {
    let r = ReceiptBuilder::new("original").backend("renamed").build();
    assert_eq!(r.backend.id, "renamed");
}
