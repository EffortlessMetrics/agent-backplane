// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for `abp_core::verify` — chain verification with parent→child relationships.

use chrono::{Duration, Utc};
use uuid::Uuid;

use abp_core::verify::{ChainBuilder, ChainError, ReceiptChain, verify_chain};
use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder};

// ── Helpers ────────────────────────────────────────────────────────

/// Build a hashed receipt with the default backend.
fn make_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

/// Build a hashed receipt with a specific start time and 1s duration.
fn make_receipt_at(started: chrono::DateTime<chrono::Utc>) -> Receipt {
    let finished = started + Duration::seconds(1);
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(started)
        .finished_at(finished)
        .with_hash()
        .unwrap()
}

/// Build a receipt with N trace events.
fn make_receipt_with_events(n: usize) -> Receipt {
    let t = Utc::now();
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..n {
        builder = builder.add_trace_event(AgentEvent {
            ts: t + Duration::milliseconds(i as i64),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    builder.with_hash().unwrap()
}

// ── Empty chain ────────────────────────────────────────────────────

#[test]
fn empty_chain_is_valid() {
    let chain = ReceiptChain::new();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 0);
    assert_eq!(result.total_events, 0);
    assert_eq!(result.total_duration_ms, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn empty_chain_via_builder() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

// ── Single receipt ─────────────────────────────────────────────────

#[test]
fn valid_single_receipt_chain() {
    let r = make_receipt();
    let chain = ChainBuilder::new().push(r).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 1);
}

#[test]
fn single_receipt_chain_length() {
    let chain = ChainBuilder::new().push(make_receipt()).build();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

// ── Valid multi-receipt chain ──────────────────────────────────────

#[test]
fn valid_multi_receipt_chain() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let r2 = make_receipt_at(t + Duration::seconds(5));
    let r3 = make_receipt_at(t + Duration::seconds(10));

    let chain = ChainBuilder::new().push(r1).push(r2).push(r3).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 3);
}

#[test]
fn valid_parent_child_relationship() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let parent_id = r1.meta.run_id;
    let r2 = make_receipt_at(t + Duration::seconds(5));

    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, parent_id)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
}

// ── Broken hash detection ──────────────────────────────────────────

#[test]
fn broken_hash_detected() {
    let mut r = make_receipt();
    r.receipt_sha256 = Some("tampered_hash_value".into());

    let chain = ChainBuilder::new().push(r).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::BrokenHash { index: 0, .. }))
    );
}

#[test]
fn broken_hash_with_valid_receipts() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let mut r2 = make_receipt_at(t + Duration::seconds(5));
    r2.receipt_sha256 = Some("bad".into());
    let r3 = make_receipt_at(t + Duration::seconds(10));

    let chain = ChainBuilder::new().push(r1).push(r2).push(r3).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    let broken: Vec<_> = result
        .errors
        .iter()
        .filter(|e| matches!(e, ChainError::BrokenHash { .. }))
        .collect();
    assert_eq!(broken.len(), 1);
    assert!(matches!(broken[0], ChainError::BrokenHash { index: 1, .. }));
}

// ── Missing parent detection ───────────────────────────────────────

#[test]
fn missing_parent_detected() {
    let r = make_receipt();
    let fake_parent = Uuid::new_v4();

    let chain = ChainBuilder::new().push_child(r, fake_parent).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ChainError::MissingParent {
            index: 0,
            parent_id
        } if *parent_id == fake_parent
    )));
}

#[test]
fn missing_parent_in_middle_of_chain() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let r2 = make_receipt_at(t + Duration::seconds(5));
    let bogus_parent = Uuid::new_v4();
    let r3 = make_receipt_at(t + Duration::seconds(10));

    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, bogus_parent)
        .push(r3)
        .build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::MissingParent { index: 1, .. }))
    );
}

// ── Out-of-order receipts ──────────────────────────────────────────

#[test]
fn out_of_order_receipts_detected() {
    let t = Utc::now();
    let r_early = make_receipt_at(t);
    let r_late = make_receipt_at(t + Duration::seconds(10));

    // Put late before early.
    let chain = ChainBuilder::new().push(r_late).push(r_early).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::OutOfOrder { index: 1 }))
    );
}

#[test]
fn out_of_order_with_parent() {
    let t = Utc::now();
    let r1 = make_receipt_at(t + Duration::seconds(10));
    let parent_id = r1.meta.run_id;
    let r2 = make_receipt_at(t); // earlier than parent

    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, parent_id)
        .build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::OutOfOrder { .. }))
    );
}

// ── Duplicate receipt IDs ──────────────────────────────────────────

#[test]
fn duplicate_receipt_ids_detected() {
    let r1 = make_receipt();
    let mut r2 = make_receipt();
    r2.meta.run_id = r1.meta.run_id;
    r2 = r2.with_hash().unwrap();

    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::DuplicateId { .. }))
    );
}

// ── Contract version mismatch ──────────────────────────────────────

#[test]
fn contract_version_mismatch_detected() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let mut r2 = make_receipt_at(t + Duration::seconds(5));
    r2.meta.contract_version = "abp/v99.0".into();
    r2 = r2.with_hash().unwrap();

    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ChainError::ContractVersionMismatch {
            index: 1,
            expected,
            actual,
        } if expected == "abp/v0.1" && actual == "abp/v99.0"
    )));
}

#[test]
fn contract_version_mismatch_multiple() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let mut r2 = make_receipt_at(t + Duration::seconds(5));
    r2.meta.contract_version = "abp/v2.0".into();
    r2 = r2.with_hash().unwrap();
    let mut r3 = make_receipt_at(t + Duration::seconds(10));
    r3.meta.contract_version = "abp/v3.0".into();
    r3 = r3.with_hash().unwrap();

    let chain = ChainBuilder::new().push(r1).push(r2).push(r3).build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    let mismatches: Vec<_> = result
        .errors
        .iter()
        .filter(|e| matches!(e, ChainError::ContractVersionMismatch { .. }))
        .collect();
    assert_eq!(mismatches.len(), 2);
}

// ── Chain statistics ───────────────────────────────────────────────

#[test]
fn chain_statistics_total_events() {
    let r1 = make_receipt_with_events(3);
    let r2 = make_receipt_with_events(5);
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert_eq!(result.total_events, 8);
}

#[test]
fn chain_statistics_duration() {
    let t = Utc::now();
    let r1 = {
        let start = t;
        let end = start + Duration::milliseconds(100);
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(end)
            .with_hash()
            .unwrap()
    };
    let r2 = {
        let start = t + Duration::seconds(1);
        let end = start + Duration::milliseconds(250);
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(end)
            .with_hash()
            .unwrap()
    };

    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert_eq!(result.total_duration_ms, 350);
}

#[test]
fn chain_statistics_zero_events_empty_traces() {
    let chain = ChainBuilder::new()
        .push(make_receipt())
        .push(make_receipt())
        .build();
    let result = verify_chain(&chain);
    assert_eq!(result.total_events, 0);
}

// ── Builder API ────────────────────────────────────────────────────

#[test]
fn builder_push_returns_self() {
    let chain = ChainBuilder::new()
        .push(make_receipt())
        .push(make_receipt())
        .push(make_receipt())
        .build();
    assert_eq!(chain.len(), 3);
}

#[test]
fn builder_push_child_returns_self() {
    let r1 = make_receipt();
    let parent_id = r1.meta.run_id;
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(make_receipt(), parent_id)
        .build();
    assert_eq!(chain.len(), 2);
}

#[test]
fn builder_empty_build() {
    let chain = ChainBuilder::new().build();
    assert!(chain.is_empty());
}

#[test]
fn builder_mixed_push_and_push_child() {
    let r1 = make_receipt();
    let parent_id = r1.meta.run_id;
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(make_receipt(), parent_id)
        .push(make_receipt())
        .build();
    assert_eq!(chain.len(), 3);
}

// ── Complex chains ─────────────────────────────────────────────────

#[test]
fn complex_chain_10_receipts() {
    let t = Utc::now();
    let mut builder = ChainBuilder::new();
    for i in 0..10 {
        let r = make_receipt_at(t + Duration::seconds(i));
        builder = builder.push(r);
    }
    let chain = builder.build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 10);
}

#[test]
fn branching_chain_multiple_children_same_parent() {
    let t = Utc::now();
    let root = make_receipt_at(t);
    let parent_id = root.meta.run_id;

    let child1 = make_receipt_at(t + Duration::seconds(1));
    let child2 = make_receipt_at(t + Duration::seconds(2));
    let child3 = make_receipt_at(t + Duration::seconds(3));

    let chain = ChainBuilder::new()
        .push(root)
        .push_child(child1, parent_id)
        .push_child(child2, parent_id)
        .push_child(child3, parent_id)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
    assert_eq!(result.chain_length, 4);
}

#[test]
fn deep_chain_grandparent_links() {
    let t = Utc::now();
    let r1 = make_receipt_at(t);
    let id1 = r1.meta.run_id;

    let r2 = make_receipt_at(t + Duration::seconds(1));
    let id2 = r2.meta.run_id;

    let r3 = make_receipt_at(t + Duration::seconds(2));

    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, id1)
        .push_child(r3, id2)
        .build();
    let result = verify_chain(&chain);
    assert!(result.valid);
}

// ── Multiple errors detected ───────────────────────────────────────

#[test]
fn multiple_errors_reported() {
    let t = Utc::now();
    // Out of order + broken hash + missing parent
    let r_late = make_receipt_at(t + Duration::seconds(10));
    let mut r_early = make_receipt_at(t);
    r_early.receipt_sha256 = Some("bad".into());
    let bogus_parent = Uuid::new_v4();

    let chain = ChainBuilder::new()
        .push(r_late)
        .push_child(r_early, bogus_parent)
        .build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    // Should have at least OutOfOrder, BrokenHash, and MissingParent
    assert!(result.errors.len() >= 3);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::OutOfOrder { .. }))
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::BrokenHash { .. }))
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ChainError::MissingParent { .. }))
    );
}

#[test]
fn all_error_types_can_coexist() {
    let t = Utc::now();
    // r1 is fine
    let r1 = make_receipt_at(t);

    // r2: duplicate ID of r1, out of order, broken hash, missing parent, version mismatch
    let mut r2 = make_receipt_at(t - Duration::seconds(5)); // out of order
    r2.meta.run_id = r1.meta.run_id; // duplicate
    r2.meta.contract_version = "abp/v9.9".into(); // version mismatch
    r2.receipt_sha256 = Some("bad".into()); // broken hash

    let bogus = Uuid::new_v4();
    let chain = ChainBuilder::new()
        .push(r1)
        .push_child(r2, bogus) // missing parent
        .build();
    let result = verify_chain(&chain);
    assert!(!result.valid);
    assert!(result.errors.len() >= 4);
}

// ── No-hash receipts are valid ─────────────────────────────────────

#[test]
fn no_hash_receipts_are_valid() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build(); // no with_hash()
    let chain = ChainBuilder::new().push(r).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
}

#[test]
fn mixed_hash_and_no_hash() {
    let t = Utc::now();
    let r1 = make_receipt_at(t); // has hash
    let r2 = {
        let start = t + Duration::seconds(5);
        let end = start + Duration::seconds(1);
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(end)
            .build() // no hash
    };
    let chain = ChainBuilder::new().push(r1).push(r2).build();
    let result = verify_chain(&chain);
    assert!(result.valid);
}

// ── Chain entries access ───────────────────────────────────────────

#[test]
fn chain_entries_returns_all() {
    let r1 = make_receipt();
    let id1 = r1.meta.run_id;
    let r2 = make_receipt();
    let id2 = r2.meta.run_id;

    let chain = ChainBuilder::new().push(r1).push_child(r2, id1).build();

    let entries = chain.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].receipt.meta.run_id, id1);
    assert!(entries[0].parent_id.is_none());
    assert_eq!(entries[1].receipt.meta.run_id, id2);
    assert_eq!(entries[1].parent_id, Some(id1));
}

#[test]
fn chain_iter_yields_entries() {
    let chain = ChainBuilder::new()
        .push(make_receipt())
        .push(make_receipt())
        .build();
    let count = chain.iter().count();
    assert_eq!(count, 2);
}

#[test]
fn chain_into_iter() {
    let chain = ChainBuilder::new()
        .push(make_receipt())
        .push(make_receipt())
        .push(make_receipt())
        .build();
    let mut count = 0;
    for _entry in &chain {
        count += 1;
    }
    assert_eq!(count, 3);
}

// ── ChainError Display ────────────────────────────────────────────

#[test]
fn chain_error_display_variants() {
    let id = Uuid::nil();

    let e = ChainError::BrokenHash {
        index: 2,
        run_id: id,
    };
    assert!(e.to_string().contains("broken hash"));
    assert!(e.to_string().contains("2"));

    let e = ChainError::MissingParent {
        index: 1,
        parent_id: id,
    };
    assert!(e.to_string().contains("missing parent"));

    let e = ChainError::OutOfOrder { index: 3 };
    assert!(e.to_string().contains("out of chronological order"));

    let e = ChainError::DuplicateId { id };
    assert!(e.to_string().contains("duplicate"));

    let e = ChainError::ContractVersionMismatch {
        index: 4,
        expected: "abp/v0.1".into(),
        actual: "abp/v0.2".into(),
    };
    let s = e.to_string();
    assert!(s.contains("abp/v0.1"));
    assert!(s.contains("abp/v0.2"));
}

// ── ChainVerification fields ───────────────────────────────────────

#[test]
fn chain_verification_valid_field() {
    let chain = ChainBuilder::new().push(make_receipt()).build();
    let v = verify_chain(&chain);
    assert!(v.valid);
    assert!(v.errors.is_empty());
}

#[test]
fn chain_verification_chain_length_field() {
    let chain = ChainBuilder::new()
        .push(make_receipt())
        .push(make_receipt())
        .push(make_receipt())
        .push(make_receipt())
        .push(make_receipt())
        .build();
    let v = verify_chain(&chain);
    assert_eq!(v.chain_length, 5);
}
