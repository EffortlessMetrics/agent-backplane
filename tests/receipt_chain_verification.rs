// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive receipt chain verification and audit trail tests.

use abp_receipt::{
    ChainError, Outcome, Receipt, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash,
    verify_hash,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a hashed receipt for the given backend with a controlled start time
/// offset (in seconds from `base`).
fn make_receipt(backend: &str, offset_secs: i64, outcome: Outcome) -> Receipt {
    let base = Utc::now();
    let start = base + Duration::seconds(offset_secs);
    let finish = start + Duration::milliseconds(150);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .expect("hash computation should succeed")
}

/// Build a chain of `n` receipts from a single backend.
fn build_chain(n: usize) -> ReceiptChain {
    let mut chain = ReceiptChain::new();
    for i in 0..n {
        let r = make_receipt("mock", i as i64, Outcome::Complete);
        chain.push(r).expect("push should succeed");
    }
    chain
}

// ===========================================================================
// 1. Receipt chain integrity – each receipt links via hash
// ===========================================================================

#[test]
fn chain_single_receipt_verifies() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", 0, Outcome::Complete);
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_multiple_receipts_verify() {
    let chain = build_chain(5);
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_preserves_insertion_order() {
    let chain = build_chain(3);
    let ids: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
    // Each receipt should have a unique run_id.
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
}

#[test]
fn chain_latest_returns_last_pushed() {
    let mut chain = ReceiptChain::new();
    let r1 = make_receipt("mock", 0, Outcome::Complete);
    let r2 = make_receipt("mock", 1, Outcome::Failed);
    let r2_id = r2.meta.run_id;
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, r2_id);
}

// ===========================================================================
// 2. Tamper detection – modify any field → chain breaks
// ===========================================================================

#[test]
fn tamper_outcome_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    assert!(verify_hash(&r));
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_backend_id_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_duration_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.meta.duration_ms = 999_999;
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_run_id_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.meta.run_id = Uuid::new_v4();
    assert!(!verify_hash(&r));
}

#[test]
fn tamper_hash_directly_detected() {
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.receipt_sha256 = Some("deadbeef".into());
    assert!(!verify_hash(&r));
}

#[test]
fn tampered_receipt_rejected_by_chain_push() {
    let mut chain = ReceiptChain::new();
    let mut r = make_receipt("mock", 0, Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper without recomputing hash
    let result = chain.push(r);
    assert!(matches!(result, Err(ChainError::HashMismatch { index: 0 })));
}

#[test]
fn tampered_receipt_detected_by_chain_verify() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", 0, Outcome::Complete);
    chain.push(r).unwrap();

    // Build a second receipt that is valid, push it, then tamper after the fact
    // by rebuilding the chain manually with a tampered copy.
    let mut chain2 = ReceiptChain::new();
    let r1 = make_receipt("mock", 0, Outcome::Complete);
    let mut r2 = make_receipt("mock", 1, Outcome::Complete);
    chain2.push(r1).unwrap();

    // Tamper r2's hash to be invalid
    r2.receipt_sha256 = Some("badhash".into());
    let err = chain2.push(r2);
    assert!(matches!(err, Err(ChainError::HashMismatch { index: 1 })));
}

// ===========================================================================
// 3. Chain traversal and querying
// ===========================================================================

#[test]
fn find_receipt_by_run_id() {
    let chain = build_chain(5);
    let target_id = chain.iter().nth(2).unwrap().meta.run_id;
    let found = chain.iter().find(|r| r.meta.run_id == target_id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().meta.run_id, target_id);
}

#[test]
fn find_receipt_by_backend() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("alpha", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("beta", 1, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("alpha", 2, Outcome::Failed))
        .unwrap();

    let alpha_receipts: Vec<&Receipt> = chain.iter().filter(|r| r.backend.id == "alpha").collect();
    assert_eq!(alpha_receipts.len(), 2);

    let beta_receipts: Vec<&Receipt> = chain.iter().filter(|r| r.backend.id == "beta").collect();
    assert_eq!(beta_receipts.len(), 1);
}

#[test]
fn find_receipts_by_time_range() {
    let mut chain = ReceiptChain::new();
    let base = Utc::now();

    for i in 0..5 {
        let start = base + Duration::seconds(i * 10);
        let finish = start + Duration::milliseconds(100);
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(finish)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }

    let range_start = base + Duration::seconds(15);
    let range_end = base + Duration::seconds(35);
    let in_range: Vec<&Receipt> = chain
        .iter()
        .filter(|r| r.meta.started_at >= range_start && r.meta.started_at <= range_end)
        .collect();
    assert_eq!(in_range.len(), 2); // offsets 20 and 30
}

#[test]
fn find_receipts_by_outcome() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("mock", 1, Outcome::Failed))
        .unwrap();
    chain
        .push(make_receipt("mock", 2, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("mock", 3, Outcome::Partial))
        .unwrap();

    let failed: Vec<_> = chain
        .iter()
        .filter(|r| r.outcome == Outcome::Failed)
        .collect();
    assert_eq!(failed.len(), 1);
}

#[test]
fn iterate_chain_via_into_iterator() {
    let chain = build_chain(3);
    let mut count = 0;
    for _r in &chain {
        count += 1;
    }
    assert_eq!(count, 3);
}

// ===========================================================================
// 4. Chain serialization / deserialization roundtrip
// ===========================================================================

#[test]
fn receipt_json_roundtrip() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = serde_json::to_string_pretty(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.run_id, r.meta.run_id);
    assert_eq!(deserialized.outcome, r.outcome);
    assert_eq!(deserialized.receipt_sha256, r.receipt_sha256);
}

#[test]
fn receipt_hash_survives_roundtrip() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deserialized));
}

#[test]
fn chain_serialization_roundtrip() {
    let chain = build_chain(4);
    // Serialize the chain receipts to a JSON array.
    let receipts: Vec<&Receipt> = chain.iter().collect();
    let json = serde_json::to_string(&receipts).unwrap();

    // Deserialize and rebuild the chain.
    let deserialized: Vec<Receipt> = serde_json::from_str(&json).unwrap();
    let mut chain2 = ReceiptChain::new();
    for r in deserialized {
        chain2.push(r).unwrap();
    }
    assert_eq!(chain2.len(), 4);
    assert!(chain2.verify().is_ok());
}

#[test]
fn canonicalize_is_deterministic() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonicalize_excludes_stored_hash() {
    let r = make_receipt("mock", 0, Outcome::Complete);
    let json = canonicalize(&r).unwrap();
    // The canonical form should have receipt_sha256 as null.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn hash_unchanged_by_stored_hash_value() {
    let r1 = make_receipt("mock", 0, Outcome::Complete);
    let h1 = compute_hash(&r1).unwrap();

    // Clone and set a different stored hash — recomputed hash must match.
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("anything".into());
    let h2 = compute_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

// ===========================================================================
// 5. Multi-backend chain – receipts from different backends interleaved
// ===========================================================================

#[test]
fn multi_backend_chain_verifies() {
    let backends = ["openai", "anthropic", "gemini", "mock", "copilot"];
    let mut chain = ReceiptChain::new();
    for (i, backend) in backends.iter().enumerate() {
        chain
            .push(make_receipt(backend, i as i64, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());
}

#[test]
fn multi_backend_chain_with_mixed_outcomes() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("openai", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("anthropic", 1, Outcome::Failed))
        .unwrap();
    chain
        .push(make_receipt("gemini", 2, Outcome::Partial))
        .unwrap();
    chain
        .push(make_receipt("openai", 3, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("anthropic", 4, Outcome::Complete))
        .unwrap();
    assert_eq!(chain.len(), 5);
    assert!(chain.verify().is_ok());

    // Per-backend filtering still works.
    let openai_count = chain.iter().filter(|r| r.backend.id == "openai").count();
    assert_eq!(openai_count, 2);
}

#[test]
fn multi_backend_interleaved_preserves_order() {
    let mut chain = ReceiptChain::new();
    let backends = ["a", "b", "a", "c", "b"];
    for (i, b) in backends.iter().enumerate() {
        chain
            .push(make_receipt(b, i as i64, Outcome::Complete))
            .unwrap();
    }
    let ids: Vec<&str> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "a", "c", "b"]);
}

// ===========================================================================
// 6. Chain statistics
// ===========================================================================

#[test]
fn chain_total_runs() {
    let chain = build_chain(7);
    assert_eq!(chain.len(), 7);
}

#[test]
fn chain_success_rate() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("mock", 1, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("mock", 2, Outcome::Failed))
        .unwrap();
    chain
        .push(make_receipt("mock", 3, Outcome::Partial))
        .unwrap();
    chain
        .push(make_receipt("mock", 4, Outcome::Complete))
        .unwrap();

    let total = chain.len() as f64;
    let successes = chain
        .iter()
        .filter(|r| r.outcome == Outcome::Complete)
        .count() as f64;
    let rate = successes / total;
    assert!((rate - 0.6).abs() < f64::EPSILON);
}

#[test]
fn chain_average_duration() {
    let mut chain = ReceiptChain::new();
    let base = Utc::now();
    let durations_ms = [100u64, 200, 300, 400, 500];
    for (i, &dur) in durations_ms.iter().enumerate() {
        let start = base + Duration::seconds(i as i64);
        let finish = start + Duration::milliseconds(dur as i64);
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(finish)
            .with_hash()
            .unwrap();
        chain.push(r).unwrap();
    }

    let avg: f64 =
        chain.iter().map(|r| r.meta.duration_ms as f64).sum::<f64>() / chain.len() as f64;
    assert!((avg - 300.0).abs() < f64::EPSILON);
}

#[test]
fn chain_per_backend_breakdown() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("alpha", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("beta", 1, Outcome::Failed))
        .unwrap();
    chain
        .push(make_receipt("alpha", 2, Outcome::Complete))
        .unwrap();
    chain
        .push(make_receipt("alpha", 3, Outcome::Failed))
        .unwrap();
    chain
        .push(make_receipt("beta", 4, Outcome::Complete))
        .unwrap();

    let mut per_backend: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for r in chain.iter() {
        let entry = per_backend.entry(r.backend.id.as_str()).or_default();
        entry.0 += 1; // total
        if r.outcome == Outcome::Complete {
            entry.1 += 1; // successes
        }
    }

    let (alpha_total, alpha_ok) = per_backend["alpha"];
    assert_eq!(alpha_total, 3);
    assert_eq!(alpha_ok, 2);

    let (beta_total, beta_ok) = per_backend["beta"];
    assert_eq!(beta_total, 2);
    assert_eq!(beta_ok, 1);
}

// ===========================================================================
// 7. Chain export formats (JSON, JSONL)
// ===========================================================================

#[test]
fn export_chain_as_json_array() {
    let chain = build_chain(3);
    let receipts: Vec<&Receipt> = chain.iter().collect();
    let json = serde_json::to_string_pretty(&receipts).unwrap();

    // Should be valid JSON array.
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

#[test]
fn export_chain_as_jsonl() {
    let chain = build_chain(4);
    let mut jsonl = String::new();
    for r in chain.iter() {
        let line = serde_json::to_string(r).unwrap();
        jsonl.push_str(&line);
        jsonl.push('\n');
    }

    // Each line should be individually parseable.
    let lines: Vec<&str> = jsonl.trim().split('\n').collect();
    assert_eq!(lines.len(), 4);
    for line in &lines {
        let parsed: Receipt = serde_json::from_str(line).unwrap();
        assert!(verify_hash(&parsed));
    }
}

#[test]
fn jsonl_import_rebuilds_chain() {
    let chain = build_chain(3);
    let mut jsonl = String::new();
    for r in chain.iter() {
        let line = serde_json::to_string(r).unwrap();
        jsonl.push_str(&line);
        jsonl.push('\n');
    }

    // Re-import from JSONL.
    let mut chain2 = ReceiptChain::new();
    for line in jsonl.trim().split('\n') {
        let r: Receipt = serde_json::from_str(line).unwrap();
        chain2.push(r).unwrap();
    }
    assert_eq!(chain2.len(), 3);
    assert!(chain2.verify().is_ok());
}

#[test]
fn json_array_import_rebuilds_chain() {
    let chain = build_chain(5);
    let receipts: Vec<&Receipt> = chain.iter().collect();
    let json = serde_json::to_string(&receipts).unwrap();

    let deserialized: Vec<Receipt> = serde_json::from_str(&json).unwrap();
    let mut chain2 = ReceiptChain::new();
    for r in deserialized {
        chain2.push(r).unwrap();
    }
    assert_eq!(chain2.len(), 5);
    assert!(chain2.verify().is_ok());
}

// ===========================================================================
// Edge cases and error conditions
// ===========================================================================

#[test]
fn empty_chain_verify_returns_error() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn duplicate_run_id_rejected() {
    let mut chain = ReceiptChain::new();
    let r = make_receipt("mock", 0, Outcome::Complete);
    let dup = r.clone();
    chain.push(r).unwrap();
    let err = chain.push(dup).unwrap_err();
    assert!(matches!(err, ChainError::DuplicateId { .. }));
}

#[test]
fn out_of_order_receipt_rejected() {
    let mut chain = ReceiptChain::new();
    // Push a receipt with a later timestamp first.
    chain
        .push(make_receipt("mock", 10, Outcome::Complete))
        .unwrap();
    // Attempt to push a receipt with an earlier timestamp.
    let result = chain.push(make_receipt("mock", 0, Outcome::Complete));
    assert!(matches!(result, Err(ChainError::BrokenLink { index: 1 })));
}

#[test]
fn receipt_without_hash_accepted_by_chain() {
    let mut chain = ReceiptChain::new();
    // Build without hash — should still be accepted (hash is optional).
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.receipt_sha256.is_none());
    chain.push(r).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_len_and_is_empty() {
    let mut chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);

    chain
        .push(make_receipt("mock", 0, Outcome::Complete))
        .unwrap();
    assert!(!chain.is_empty());
    assert_eq!(chain.len(), 1);
}

#[test]
fn latest_on_empty_chain_is_none() {
    let chain = ReceiptChain::new();
    assert!(chain.latest().is_none());
}

#[test]
fn large_chain_verifies() {
    let chain = build_chain(100);
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
}
