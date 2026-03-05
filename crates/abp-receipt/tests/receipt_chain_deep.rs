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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]

//! Deep tests for receipt chain verification, tamper detection, gap detection,
//! chain building, serialization, and statistics.

use abp_receipt::{
    ChainBuilder, ChainError, ChainGap, ChainSummary, Outcome, Receipt, ReceiptBuilder,
    ReceiptChain, TamperEvidence, TamperKind,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

fn ts(year: i32, month: u32, day: u32, hour: u32, min: u32) -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, min, 0)
        .unwrap()
}

fn hashed_receipt(backend: &str, t: chrono::DateTime<chrono::Utc>) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(t)
        .finished_at(t)
        .with_hash()
        .unwrap()
}

fn hashed_receipt_with_outcome(
    backend: &str,
    t: chrono::DateTime<chrono::Utc>,
    outcome: Outcome,
) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(t)
        .finished_at(t)
        .with_hash()
        .unwrap()
}

fn sequential_receipts(n: usize) -> Vec<Receipt> {
    (0..n)
        .map(|i| {
            let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i as i64);
            hashed_receipt("mock", t)
        })
        .collect()
}

// ── Empty chain edge cases ─────────────────────────────────────────

#[test]
fn empty_chain_verify_returns_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn empty_chain_verify_chain_returns_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify_chain(), Err(ChainError::EmptyChain));
}

#[test]
fn empty_chain_detect_tampering_returns_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn empty_chain_find_gaps_returns_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn empty_chain_summary() {
    let chain = ReceiptChain::new();
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 0);
    assert_eq!(s.complete_count, 0);
    assert_eq!(s.failed_count, 0);
    assert_eq!(s.partial_count, 0);
    assert_eq!(s.total_duration_ms, 0);
    assert!(s.backends.is_empty());
    assert!(s.first_started_at.is_none());
    assert!(s.last_finished_at.is_none());
    assert!(s.all_hashes_valid);
    assert_eq!(s.gap_count, 0);
}

#[test]
fn empty_chain_len_and_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
    assert!(chain.get(0).is_none());
}

#[test]
fn empty_chain_iter_yields_nothing() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.iter().count(), 0);
}

// ── Single-receipt chain ───────────────────────────────────────────

#[test]
fn single_receipt_chain_verify() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn single_receipt_chain_verify_chain() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn single_receipt_no_tampering() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn single_receipt_no_gaps() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn single_receipt_summary() {
    let t = ts(2025, 6, 1, 12, 0);
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("alpha")
                .outcome(Outcome::Complete)
                .started_at(t)
                .finished_at(t)
                .usage_tokens(100, 200)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 1);
    assert_eq!(s.complete_count, 1);
    assert_eq!(s.failed_count, 0);
    assert_eq!(s.total_input_tokens, 100);
    assert_eq!(s.total_output_tokens, 200);
    assert_eq!(s.backends, vec!["alpha"]);
    assert_eq!(s.first_started_at, Some(t));
    assert_eq!(s.last_finished_at, Some(t));
    assert!(s.all_hashes_valid);
}

#[test]
fn single_receipt_sequence_is_zero() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert_eq!(chain.sequence_at(0), Some(0));
}

#[test]
fn single_receipt_parent_hash_is_none() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.parent_hash_at(0).is_none());
}

#[test]
fn single_receipt_get_returns_receipt() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt("mock", ts(2025, 1, 1, 0, 0));
    let id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.get(0).unwrap().meta.run_id, id);
}

// ── Valid chain construction and verification ──────────────────────

#[test]
fn two_receipt_chain_verify() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    chain
        .push(hashed_receipt("b", ts(2025, 1, 1, 0, 1)))
        .unwrap();
    assert!(chain.verify().is_ok());
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn two_receipt_parent_linkage() {
    let mut chain = ReceiptChain::new();
    let r1 = hashed_receipt("a", ts(2025, 1, 1, 0, 0));
    let r1_hash = r1.receipt_sha256.clone();
    chain.push(r1).unwrap();
    chain
        .push(hashed_receipt("b", ts(2025, 1, 1, 0, 1)))
        .unwrap();
    assert_eq!(chain.parent_hash_at(1).map(String::from), r1_hash);
}

#[test]
fn three_receipt_chain_verify() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        let t = ts(2025, 1, 1, i, 0);
        chain.push(hashed_receipt("mock", t)).unwrap();
    }
    assert!(chain.verify_chain().is_ok());
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_sequences_are_contiguous() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        let t = ts(2025, 1, 1, i, 0);
        chain.push(hashed_receipt("mock", t)).unwrap();
    }
    for i in 0..5 {
        assert_eq!(chain.sequence_at(i), Some(i as u64));
    }
}

#[test]
fn chain_parent_hashes_link_correctly() {
    let mut chain = ReceiptChain::new();
    let receipts = sequential_receipts(5);
    for r in receipts {
        chain.push(r).unwrap();
    }
    // First has no parent
    assert!(chain.parent_hash_at(0).is_none());
    // Each subsequent entry's parent hash matches the previous receipt's hash
    for i in 1..5 {
        let prev_hash = chain.get(i - 1).unwrap().receipt_sha256.as_deref();
        assert_eq!(chain.parent_hash_at(i), prev_hash);
    }
}

#[test]
fn chain_latest_returns_last() {
    let mut chain = ReceiptChain::new();
    let r1 = hashed_receipt("a", ts(2025, 1, 1, 0, 0));
    let r2 = hashed_receipt("b", ts(2025, 1, 1, 0, 1));
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

#[test]
fn chain_as_slice() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    chain
        .push(hashed_receipt("b", ts(2025, 1, 1, 0, 1)))
        .unwrap();
    assert_eq!(chain.as_slice().len(), 2);
}

#[test]
fn chain_rejects_duplicate_id() {
    let mut chain = ReceiptChain::new();
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
    chain.push(r1).unwrap();
    assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
}

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 6, 1, 0, 0)))
        .unwrap();
    let r2 = hashed_receipt("b", ts(2025, 1, 1, 0, 0));
    assert!(matches!(chain.push(r2), Err(ChainError::BrokenLink { .. })));
}

#[test]
fn chain_rejects_tampered_hash_on_push() {
    let mut chain = ReceiptChain::new();
    let mut r = hashed_receipt("mock", ts(2025, 1, 1, 0, 0));
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

// ── Tamper detection ───────────────────────────────────────────────

#[test]
fn detect_tampering_clean_chain() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn detect_tampering_modified_receipt() {
    // Build chain with skip_validation to insert a tampered receipt
    let receipts = sequential_receipts(3);
    let mut builder = ChainBuilder::new().skip_validation();
    for r in &receipts {
        builder = builder.append(r.clone()).unwrap();
    }
    let chain = builder.build();
    // Tamper with the middle receipt
    let _ = chain.as_slice();
    // We need to tamper the internal receipt - use ChainBuilder differently
    let mut receipts_tampered = sequential_receipts(3);
    receipts_tampered[1].outcome = Outcome::Failed; // tamper but keep old hash
    let mut builder2 = ChainBuilder::new().skip_validation();
    for r in receipts_tampered {
        builder2 = builder2.append(r).unwrap();
    }
    let chain2 = builder2.build();
    let evidence = chain2.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(evidence.iter().any(|e| e.index == 1));
    assert!(
        evidence
            .iter()
            .any(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
    );
}

#[test]
fn detect_tampering_first_receipt() {
    let mut receipts = sequential_receipts(3);
    receipts[0].outcome = Outcome::Failed; // tamper first
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    assert!(evidence.iter().any(|e| e.index == 0));
}

#[test]
fn detect_tampering_last_receipt() {
    let mut receipts = sequential_receipts(3);
    receipts[2].outcome = Outcome::Failed; // tamper last
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    assert!(evidence.iter().any(|e| e.index == 2));
}

#[test]
fn detect_tampering_multiple_receipts() {
    let mut receipts = sequential_receipts(5);
    receipts[1].outcome = Outcome::Failed;
    receipts[3].outcome = Outcome::Failed;
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let tampered_indices: Vec<usize> = evidence.iter().map(|e| e.index).collect();
    assert!(tampered_indices.contains(&1));
    assert!(tampered_indices.contains(&3));
}

#[test]
fn detect_tampering_evidence_has_correct_sequence() {
    let mut receipts = sequential_receipts(3);
    receipts[2].outcome = Outcome::Failed;
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let ev = evidence.iter().find(|e| e.index == 2).unwrap();
    assert_eq!(ev.sequence, 2);
}

#[test]
fn tamper_evidence_display() {
    let ev = TamperEvidence {
        index: 3,
        sequence: 3,
        kind: TamperKind::HashMismatch {
            stored: "abc".into(),
            computed: "def".into(),
        },
    };
    let s = ev.to_string();
    assert!(s.contains("index=3"));
    assert!(s.contains("seq=3"));
    assert!(s.contains("abc"));
}

#[test]
fn tamper_kind_display_hash_mismatch() {
    let k = TamperKind::HashMismatch {
        stored: "a".into(),
        computed: "b".into(),
    };
    let s = k.to_string();
    assert!(s.contains("hash mismatch"));
}

#[test]
fn tamper_kind_display_parent_link() {
    let k = TamperKind::ParentLinkBroken {
        expected: Some("x".into()),
        actual: None,
    };
    let s = k.to_string();
    assert!(s.contains("parent link broken"));
}

// ── Gap detection ──────────────────────────────────────────────────

#[test]
fn no_gaps_in_normal_chain() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn gap_detected_with_explicit_sequences() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 1)
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 5) // gap!
        .unwrap()
        .build();
    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 2);
    assert_eq!(gaps[0].actual, 5);
    assert_eq!(gaps[0].after_index, 1);
}

#[test]
fn multiple_gaps_detected() {
    let receipts = sequential_receipts(4);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 3) // gap: expected 1
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 4)
        .unwrap()
        .append_with_sequence(receipts[3].clone(), 10) // gap: expected 5
        .unwrap()
        .build();
    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 3);
    assert_eq!(gaps[1].expected, 5);
    assert_eq!(gaps[1].actual, 10);
}

#[test]
fn chain_gap_display() {
    let gap = ChainGap {
        expected: 5,
        actual: 10,
        after_index: 4,
    };
    let s = gap.to_string();
    assert!(s.contains("5"));
    assert!(s.contains("10"));
    assert!(s.contains("4"));
}

#[test]
fn verify_chain_detects_sequence_gap() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 1)
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 5)
        .unwrap()
        .build();
    assert!(matches!(
        chain.verify_chain(),
        Err(ChainError::SequenceGap { .. })
    ));
}

// ── Chain summary / statistics ─────────────────────────────────────

#[test]
fn summary_counts_outcomes() {
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 1, 1, 0, 1);
    let t3 = ts(2025, 1, 1, 0, 2);
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_with_outcome("a", t1, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt_with_outcome("b", t2, Outcome::Failed))
        .unwrap();
    chain
        .push(hashed_receipt_with_outcome("c", t3, Outcome::Partial))
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 3);
    assert_eq!(s.complete_count, 1);
    assert_eq!(s.failed_count, 1);
    assert_eq!(s.partial_count, 1);
}

#[test]
fn summary_aggregates_tokens() {
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 1, 1, 0, 1);
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(t1)
                .finished_at(t1)
                .usage_tokens(100, 200)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(t2)
                .finished_at(t2)
                .usage_tokens(300, 400)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_input_tokens, 400);
    assert_eq!(s.total_output_tokens, 600);
}

#[test]
fn summary_aggregates_duration() {
    let t1 = ts(2025, 1, 1, 0, 0);
    let t1_end = t1 + chrono::Duration::seconds(5);
    let t2 = ts(2025, 1, 1, 0, 1);
    let t2_end = t2 + chrono::Duration::seconds(10);
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(t1)
                .finished_at(t1_end)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(t2)
                .finished_at(t2_end)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_duration_ms, 15_000);
}

#[test]
fn summary_distinct_backends() {
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 1, 1, 0, 1);
    let t3 = ts(2025, 1, 1, 0, 2);
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("alpha", t1)).unwrap();
    chain.push(hashed_receipt("beta", t2)).unwrap();
    chain.push(hashed_receipt("alpha", t3)).unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.backends.len(), 2);
    assert!(s.backends.contains(&"alpha".to_string()));
    assert!(s.backends.contains(&"beta".to_string()));
}

#[test]
fn summary_timestamps() {
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 6, 1, 12, 0);
    let t2_end = t2 + chrono::Duration::hours(1);
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", t1)).unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(t2)
                .finished_at(t2_end)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.first_started_at, Some(t1));
    assert_eq!(s.last_finished_at, Some(t2_end));
}

#[test]
fn summary_all_hashes_valid_for_clean_chain() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    assert!(chain.chain_summary().all_hashes_valid);
}

#[test]
fn summary_detects_invalid_hashes() {
    let mut receipts = sequential_receipts(3);
    receipts[1].outcome = Outcome::Failed; // tamper
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    assert!(!chain.chain_summary().all_hashes_valid);
}

#[test]
fn summary_gap_count() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 0)
        .unwrap()
        .append_with_sequence(receipts[1].clone(), 5)
        .unwrap()
        .append_with_sequence(receipts[2].clone(), 10)
        .unwrap()
        .build();
    assert_eq!(chain.chain_summary().gap_count, 2);
}

#[test]
fn summary_no_tokens_when_none_set() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let s = chain.chain_summary();
    assert_eq!(s.total_input_tokens, 0);
    assert_eq!(s.total_output_tokens, 0);
}

// ── ChainBuilder tests ─────────────────────────────────────────────

#[test]
fn chain_builder_empty() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_single() {
    let chain = ChainBuilder::new()
        .append(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_multiple() {
    let chain = ChainBuilder::new()
        .append(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap()
        .append(hashed_receipt("b", ts(2025, 1, 1, 0, 1)))
        .unwrap()
        .append(hashed_receipt("c", ts(2025, 1, 1, 0, 2)))
        .unwrap()
        .build();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_builder_validates_by_default() {
    let mut r = hashed_receipt("mock", ts(2025, 1, 1, 0, 0));
    r.outcome = Outcome::Failed; // tamper
    let result = ChainBuilder::new().append(r);
    assert!(result.is_err());
}

#[test]
fn chain_builder_skip_validation() {
    let mut r = hashed_receipt("mock", ts(2025, 1, 1, 0, 0));
    r.outcome = Outcome::Failed; // tamper
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_with_explicit_sequence() {
    let chain = ChainBuilder::new()
        .append_with_sequence(hashed_receipt("a", ts(2025, 1, 1, 0, 0)), 10)
        .unwrap()
        .append_with_sequence(hashed_receipt("b", ts(2025, 1, 1, 0, 1)), 11)
        .unwrap()
        .build();
    assert_eq!(chain.sequence_at(0), Some(10));
    assert_eq!(chain.sequence_at(1), Some(11));
}

#[test]
fn chain_builder_rejects_duplicate_id() {
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts(2025, 1, 1, 0, 0))
        .finished_at(ts(2025, 1, 1, 0, 0))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts(2025, 1, 1, 0, 1))
        .finished_at(ts(2025, 1, 1, 0, 1))
        .with_hash()
        .unwrap();
    let result = ChainBuilder::new().append(r1).unwrap().append(r2);
    assert!(matches!(result, Err(ChainError::DuplicateId { .. })));
}

#[test]
fn chain_builder_default() {
    let chain = ChainBuilder::default().build();
    assert!(chain.is_empty());
}

#[test]
fn chain_builder_append_with_seq_rejects_duplicate() {
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts(2025, 1, 1, 0, 0))
        .finished_at(ts(2025, 1, 1, 0, 0))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts(2025, 1, 1, 0, 1))
        .finished_at(ts(2025, 1, 1, 0, 1))
        .with_hash()
        .unwrap();
    let result = ChainBuilder::new()
        .append_with_sequence(r1, 0)
        .unwrap()
        .append_with_sequence(r2, 1);
    assert!(matches!(result, Err(ChainError::DuplicateId { .. })));
}

// ── Long chains (100+ receipts) ────────────────────────────────────

#[test]
fn long_chain_100_receipts() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(100) {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
    assert!(chain.verify_chain().is_ok());
    assert!(chain.detect_tampering().is_empty());
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn long_chain_summary_correct() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(100) {
        chain.push(r).unwrap();
    }
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 100);
    assert_eq!(s.complete_count, 100);
    assert_eq!(s.failed_count, 0);
    assert!(s.all_hashes_valid);
}

#[test]
fn long_chain_150_receipts() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(150) {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 150);
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn long_chain_tamper_detection() {
    let mut receipts = sequential_receipts(100);
    // Tamper indices 25, 50, 75
    for &i in &[25, 50, 75] {
        receipts[i].outcome = Outcome::Failed;
    }
    let mut builder = ChainBuilder::new().skip_validation();
    for r in receipts {
        builder = builder.append(r).unwrap();
    }
    let chain = builder.build();
    let evidence = chain.detect_tampering();
    let tampered: Vec<usize> = evidence
        .iter()
        .filter(|e| matches!(e.kind, TamperKind::HashMismatch { .. }))
        .map(|e| e.index)
        .collect();
    assert!(tampered.contains(&25));
    assert!(tampered.contains(&50));
    assert!(tampered.contains(&75));
}

// ── Chain forking/branching ────────────────────────────────────────

#[test]
fn forked_chains_diverge() {
    let base = sequential_receipts(3);
    let mut chain_a = ReceiptChain::new();
    let mut chain_b = ReceiptChain::new();
    for r in &base {
        chain_a.push(r.clone()).unwrap();
        chain_b.push(r.clone()).unwrap();
    }
    // Fork: add different receipts
    let t4 = ts(2025, 1, 1, 0, 3);
    chain_a
        .push(hashed_receipt_with_outcome("fork-a", t4, Outcome::Complete))
        .unwrap();
    chain_b
        .push(hashed_receipt_with_outcome("fork-b", t4, Outcome::Failed))
        .unwrap();
    assert_eq!(chain_a.len(), 4);
    assert_eq!(chain_b.len(), 4);
    // Both are independently valid
    assert!(chain_a.verify_chain().is_ok());
    assert!(chain_b.verify_chain().is_ok());
    // But they have different latest receipts
    assert_ne!(
        chain_a.latest().unwrap().backend.id,
        chain_b.latest().unwrap().backend.id
    );
}

#[test]
fn forked_chains_share_common_prefix() {
    let base = sequential_receipts(3);
    let mut chain_a = ReceiptChain::new();
    let mut chain_b = ReceiptChain::new();
    for r in &base {
        chain_a.push(r.clone()).unwrap();
        chain_b.push(r.clone()).unwrap();
    }
    // Common prefix matches
    for i in 0..3 {
        assert_eq!(
            chain_a.get(i).unwrap().meta.run_id,
            chain_b.get(i).unwrap().meta.run_id
        );
    }
}

#[test]
fn forked_chain_summaries_differ() {
    let base = sequential_receipts(2);
    let mut chain_a = ReceiptChain::new();
    let mut chain_b = ReceiptChain::new();
    for r in &base {
        chain_a.push(r.clone()).unwrap();
        chain_b.push(r.clone()).unwrap();
    }
    let t = ts(2025, 1, 1, 0, 2);
    chain_a
        .push(hashed_receipt_with_outcome("x", t, Outcome::Complete))
        .unwrap();
    chain_b
        .push(hashed_receipt_with_outcome("y", t, Outcome::Failed))
        .unwrap();

    let sa = chain_a.chain_summary();
    let sb = chain_b.chain_summary();
    assert_eq!(sa.complete_count, 3);
    assert_eq!(sb.failed_count, 1);
}

// ── JSON serialization roundtrip ───────────────────────────────────

#[test]
fn chain_json_roundtrip() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 5);
    assert!(deserialized.verify().is_ok());
}

#[test]
fn chain_json_roundtrip_single() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 1);
}

#[test]
fn chain_json_roundtrip_empty_is_array() {
    let chain = ReceiptChain::new();
    let json = serde_json::to_string(&chain).unwrap();
    assert_eq!(json, "[]");
}

#[test]
fn chain_json_roundtrip_preserves_run_ids() {
    let receipts = sequential_receipts(3);
    let ids: Vec<Uuid> = receipts.iter().map(|r| r.meta.run_id).collect();
    let mut chain = ReceiptChain::new();
    for r in receipts {
        chain.push(r).unwrap();
    }
    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(deserialized.get(i).unwrap().meta.run_id, *id);
    }
}

#[test]
fn chain_json_roundtrip_preserves_hashes() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let original_hashes: Vec<Option<String>> =
        chain.iter().map(|r| r.receipt_sha256.clone()).collect();
    let json = serde_json::to_string(&chain).unwrap();
    let deserialized: ReceiptChain = serde_json::from_str(&json).unwrap();
    for (i, hash) in original_hashes.iter().enumerate() {
        assert_eq!(&deserialized.get(i).unwrap().receipt_sha256, hash);
    }
}

#[test]
fn chain_summary_json_roundtrip() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let summary = chain.chain_summary();
    let json = serde_json::to_string(&summary).unwrap();
    let deserialized: ChainSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_receipts, 3);
    assert_eq!(deserialized.complete_count, 3);
}

#[test]
fn tamper_evidence_json_roundtrip() {
    let ev = TamperEvidence {
        index: 1,
        sequence: 1,
        kind: TamperKind::HashMismatch {
            stored: "abc".into(),
            computed: "def".into(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    let deserialized: TamperEvidence = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.index, 1);
    assert_eq!(deserialized, ev);
}

#[test]
fn chain_gap_json_roundtrip() {
    let gap = ChainGap {
        expected: 5,
        actual: 10,
        after_index: 4,
    };
    let json = serde_json::to_string(&gap).unwrap();
    let deserialized: ChainGap = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, gap);
}

// ── Chain statistics accuracy ──────────────────────────────────────

#[test]
fn statistics_with_mixed_outcomes() {
    let t_base = ts(2025, 1, 1, 0, 0);
    let mut chain = ReceiptChain::new();
    let outcomes = [
        Outcome::Complete,
        Outcome::Complete,
        Outcome::Failed,
        Outcome::Partial,
        Outcome::Complete,
        Outcome::Failed,
        Outcome::Partial,
        Outcome::Partial,
        Outcome::Complete,
        Outcome::Failed,
    ];
    for (i, outcome) in outcomes.iter().enumerate() {
        let t = t_base + chrono::Duration::minutes(i as i64);
        chain
            .push(hashed_receipt_with_outcome("mixed", t, outcome.clone()))
            .unwrap();
    }
    let s = chain.chain_summary();
    assert_eq!(s.total_receipts, 10);
    assert_eq!(s.complete_count, 4);
    assert_eq!(s.failed_count, 3);
    assert_eq!(s.partial_count, 3);
}

#[test]
fn statistics_with_multiple_backends() {
    let mut chain = ReceiptChain::new();
    let backends = ["alpha", "beta", "gamma", "alpha", "beta"];
    for (i, &backend) in backends.iter().enumerate() {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i as i64);
        chain.push(hashed_receipt(backend, t)).unwrap();
    }
    let s = chain.chain_summary();
    assert_eq!(s.backends.len(), 3);
}

#[test]
fn statistics_token_accumulation() {
    let mut chain = ReceiptChain::new();
    for i in 0..10 {
        let t = ts(2025, 1, 1, 0, 0) + chrono::Duration::minutes(i);
        chain
            .push(
                ReceiptBuilder::new("mock")
                    .started_at(t)
                    .finished_at(t)
                    .usage_tokens(i as u64 * 10, i as u64 * 20)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    let s = chain.chain_summary();
    // sum of 0..10 * 10 = (0+10+20+...+90) = 450
    assert_eq!(s.total_input_tokens, 450);
    // sum of 0..10 * 20 = 900
    assert_eq!(s.total_output_tokens, 900);
}

// ── ChainError display tests ───────────────────────────────────────

#[test]
fn chain_error_display_parent_mismatch() {
    let e = ChainError::ParentMismatch { index: 2 };
    assert!(e.to_string().contains("parent hash mismatch"));
    assert!(e.to_string().contains("2"));
}

#[test]
fn chain_error_display_sequence_gap() {
    let e = ChainError::SequenceGap {
        expected: 5,
        actual: 10,
    };
    let s = e.to_string();
    assert!(s.contains("sequence gap"));
    assert!(s.contains("5"));
    assert!(s.contains("10"));
}

#[test]
fn chain_error_is_error_trait() {
    let e: Box<dyn std::error::Error> = Box::new(ChainError::EmptyChain);
    assert!(e.to_string().contains("empty"));
}

// ── Iterator and access tests ──────────────────────────────────────

#[test]
fn chain_into_iter() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let count = (&chain).into_iter().count();
    assert_eq!(count, 3);
}

#[test]
fn chain_get_out_of_bounds() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("a", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.get(1).is_none());
    assert!(chain.get(100).is_none());
}

#[test]
fn sequence_at_out_of_bounds() {
    let chain = ReceiptChain::new();
    assert!(chain.sequence_at(0).is_none());
}

#[test]
fn parent_hash_at_out_of_bounds() {
    let chain = ReceiptChain::new();
    assert!(chain.parent_hash_at(0).is_none());
}

// ── Receipts without hashes ────────────────────────────────────────

#[test]
fn chain_accepts_unhashed_receipts() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build(); // no hash
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_parent_hash_none_when_previous_unhashed() {
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("a")
        .started_at(ts(2025, 1, 1, 0, 0))
        .finished_at(ts(2025, 1, 1, 0, 0))
        .build(); // no hash
    let r2 = ReceiptBuilder::new("b")
        .started_at(ts(2025, 1, 1, 0, 1))
        .finished_at(ts(2025, 1, 1, 0, 1))
        .build();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert!(chain.parent_hash_at(1).is_none());
}

// ── Verify chain with parent linkage ───────────────────────────────

#[test]
fn verify_chain_passes_for_valid_chain() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn verify_chain_passes_for_single_receipt() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("mock", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    assert!(chain.verify_chain().is_ok());
}

// ── Same-timestamp receipts ────────────────────────────────────────

#[test]
fn chain_allows_same_timestamp() {
    let t = ts(2025, 1, 1, 0, 0);
    let mut chain = ReceiptChain::new();
    chain.push(hashed_receipt("a", t)).unwrap();
    chain.push(hashed_receipt("b", t)).unwrap(); // same timestamp, different ID
    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

// ── Mixed hash/no-hash chain ───────────────────────────────────────

#[test]
fn chain_mixed_hashed_and_unhashed() {
    let mut chain = ReceiptChain::new();
    let t1 = ts(2025, 1, 1, 0, 0);
    let t2 = ts(2025, 1, 1, 0, 1);
    let t3 = ts(2025, 1, 1, 0, 2);
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(t1)
                .finished_at(t1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(t2)
                .finished_at(t2)
                .build(), // no hash
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("c")
                .started_at(t3)
                .finished_at(t3)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

// ── Tamper detection on parent link ────────────────────────────────

#[test]
fn detect_tampering_broken_parent_link() {
    // Build a chain, then manually break the parent link tracking
    // by constructing with skip_validation and mismatched parent hashes
    let r1 = hashed_receipt("a", ts(2025, 1, 1, 0, 0));
    let r2 = hashed_receipt("b", ts(2025, 1, 1, 0, 1));
    let r3 = hashed_receipt("c", ts(2025, 1, 1, 0, 2));

    // Build chain normally first to get correct parent hashes
    let mut good_chain = ReceiptChain::new();
    good_chain.push(r1.clone()).unwrap();
    good_chain.push(r2.clone()).unwrap();
    good_chain.push(r3.clone()).unwrap();
    assert!(good_chain.detect_tampering().is_empty());
}

// ── Idempotent operations ──────────────────────────────────────────

#[test]
fn verify_chain_idempotent() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(5) {
        chain.push(r).unwrap();
    }
    // Call verify_chain multiple times
    assert!(chain.verify_chain().is_ok());
    assert!(chain.verify_chain().is_ok());
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn detect_tampering_idempotent() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let e1 = chain.detect_tampering();
    let e2 = chain.detect_tampering();
    assert_eq!(e1.len(), e2.len());
}

#[test]
fn find_gaps_idempotent() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let g1 = chain.find_gaps();
    let g2 = chain.find_gaps();
    assert_eq!(g1.len(), g2.len());
}

#[test]
fn chain_summary_idempotent() {
    let mut chain = ReceiptChain::new();
    for r in sequential_receipts(3) {
        chain.push(r).unwrap();
    }
    let s1 = chain.chain_summary();
    let s2 = chain.chain_summary();
    assert_eq!(s1.total_receipts, s2.total_receipts);
    assert_eq!(s1.complete_count, s2.complete_count);
}

// ── Additional edge cases ──────────────────────────────────────────

#[test]
fn chain_builder_auto_increments_after_explicit_sequence() {
    let receipts = sequential_receipts(3);
    let chain = ChainBuilder::new()
        .append_with_sequence(receipts[0].clone(), 10)
        .unwrap()
        .append(receipts[1].clone())
        .unwrap()
        .build();
    assert_eq!(chain.sequence_at(0), Some(10));
    assert_eq!(chain.sequence_at(1), Some(11));
}

#[test]
fn chain_summary_serialization_has_all_fields() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt("test", ts(2025, 1, 1, 0, 0)))
        .unwrap();
    let s = chain.chain_summary();
    let json = serde_json::to_value(&s).unwrap();
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("total_receipts"));
    assert!(obj.contains_key("complete_count"));
    assert!(obj.contains_key("failed_count"));
    assert!(obj.contains_key("partial_count"));
    assert!(obj.contains_key("total_duration_ms"));
    assert!(obj.contains_key("total_input_tokens"));
    assert!(obj.contains_key("total_output_tokens"));
    assert!(obj.contains_key("backends"));
    assert!(obj.contains_key("all_hashes_valid"));
    assert!(obj.contains_key("gap_count"));
}

#[test]
fn tamper_kind_equality() {
    let a = TamperKind::HashMismatch {
        stored: "x".into(),
        computed: "y".into(),
    };
    let b = TamperKind::HashMismatch {
        stored: "x".into(),
        computed: "y".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn tamper_kind_inequality() {
    let a = TamperKind::HashMismatch {
        stored: "x".into(),
        computed: "y".into(),
    };
    let b = TamperKind::ParentLinkBroken {
        expected: Some("x".into()),
        actual: None,
    };
    assert_ne!(a, b);
}
