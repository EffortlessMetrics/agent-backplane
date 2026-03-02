// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for receipt storage, retrieval, chain management,
//! and integrity verification via `ReceiptStore` (file-based) and
//! `ReceiptChain` (in-memory ordered chain).

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, Outcome, Receipt, UsageNormalized, VerificationReport,
    receipt_hash,
};
use abp_receipt::{
    ChainError, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash, diff_receipts,
    verify_hash,
};
use abp_runtime::store::ReceiptStore;
use chrono::{DateTime, Duration, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()
}

fn time_offset(secs: i64) -> DateTime<Utc> {
    base_time() + Duration::seconds(secs)
}

/// Build a hashed receipt at a given second-offset from `base_time`.
fn hashed_receipt_at(backend: &str, offset_secs: i64, outcome: Outcome) -> Receipt {
    let start = time_offset(offset_secs);
    let finish = start + Duration::milliseconds(100);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap()
}

/// Build a hashed receipt with a fixed run_id.
fn hashed_receipt_with_id(id: Uuid, offset_secs: i64) -> Receipt {
    let start = time_offset(offset_secs);
    let finish = start + Duration::milliseconds(50);
    ReceiptBuilder::new("mock")
        .run_id(id)
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap()
}

/// Build a plain (unhashed) receipt at a given offset.
fn plain_receipt_at(backend: &str, offset_secs: i64) -> Receipt {
    let start = time_offset(offset_secs);
    let finish = start + Duration::milliseconds(100);
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(finish)
        .build()
}

// ===========================================================================
// ReceiptChain — construction
// ===========================================================================

#[test]
fn chain_default_is_empty() {
    let chain = ReceiptChain::default();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
}

#[test]
fn chain_new_latest_is_none() {
    let chain = ReceiptChain::new();
    assert!(chain.latest().is_none());
}

#[test]
fn chain_new_iter_is_empty() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.iter().count(), 0);
}

// ===========================================================================
// ReceiptChain — store and retrieve single receipt
// ===========================================================================

#[test]
fn chain_push_single_receipt() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt_at("mock", 0, Outcome::Complete);
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn chain_latest_after_single_push() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt_at("mock", 0, Outcome::Complete);
    let id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, id);
}

#[test]
fn chain_iter_single_element() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt_at("backend-a", 0, Outcome::Complete);
    chain.push(r).unwrap();
    let backends: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(backends, vec!["backend-a"]);
}

#[test]
fn chain_verify_single_hashed() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("m", 0, Outcome::Complete))
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_push_unhashed_receipt_succeeds() {
    let mut chain = ReceiptChain::new();
    let r = plain_receipt_at("mock", 0);
    assert!(r.receipt_sha256.is_none());
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

// ===========================================================================
// ReceiptChain — store multiple receipts
// ===========================================================================

#[test]
fn chain_push_two_chronological() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("a", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt_at("b", 10, Outcome::Partial))
        .unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_push_three_chronological() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(hashed_receipt_at("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_push_ten_receipts() {
    let mut chain = ReceiptChain::new();
    for i in 0..10 {
        chain
            .push(hashed_receipt_at("m", i * 5, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 10);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_latest_is_last_pushed() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("first", 0, Outcome::Complete))
        .unwrap();
    let last = hashed_receipt_at("last", 100, Outcome::Failed);
    let last_id = last.meta.run_id;
    chain.push(last).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, last_id);
}

#[test]
fn chain_iter_preserves_order() {
    let mut chain = ReceiptChain::new();
    let tags = ["alpha", "beta", "gamma"];
    for (i, tag) in tags.iter().enumerate() {
        chain
            .push(hashed_receipt_at(tag, (i as i64) * 10, Outcome::Complete))
            .unwrap();
    }
    let names: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn chain_verify_multiple_all_hashed() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(hashed_receipt_at("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_into_iterator() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("x", 0, Outcome::Complete))
        .unwrap();
    chain
        .push(hashed_receipt_at("y", 10, Outcome::Complete))
        .unwrap();
    let count = (&chain).into_iter().count();
    assert_eq!(count, 2);
}

// ===========================================================================
// ReceiptChain — empty chain behavior
// ===========================================================================

#[test]
fn chain_verify_empty_is_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_is_empty_true_for_new() {
    assert!(ReceiptChain::new().is_empty());
}

#[test]
fn chain_len_zero_for_new() {
    assert_eq!(ReceiptChain::new().len(), 0);
}

// ===========================================================================
// ReceiptChain — duplicate ID rejection
// ===========================================================================

#[test]
fn chain_rejects_duplicate_run_id() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    chain.push(hashed_receipt_with_id(id, 0)).unwrap();
    let dup = hashed_receipt_with_id(id, 10);
    assert_eq!(chain.push(dup), Err(ChainError::DuplicateId { id }));
}

#[test]
fn chain_duplicate_leaves_chain_unchanged() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    chain.push(hashed_receipt_with_id(id, 0)).unwrap();
    let _ = chain.push(hashed_receipt_with_id(id, 10));
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_unique_ids_accepted() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(hashed_receipt_with_id(Uuid::new_v4(), i * 10))
            .unwrap();
    }
    assert_eq!(chain.len(), 5);
}

// ===========================================================================
// ReceiptChain — hash verification
// ===========================================================================

#[test]
fn chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = hashed_receipt_at("m", 0, Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper after hashing
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_rejects_garbage_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = plain_receipt_at("m", 0);
    r.receipt_sha256 = Some("not_a_valid_hash".into());
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_accepts_none_hash() {
    let mut chain = ReceiptChain::new();
    let r = plain_receipt_at("m", 0);
    assert!(r.receipt_sha256.is_none());
    chain.push(r).unwrap();
}

// ===========================================================================
// ReceiptChain — chronological ordering (broken link)
// ===========================================================================

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("m", 100, Outcome::Complete))
        .unwrap();
    let earlier = hashed_receipt_at("m", 0, Outcome::Complete);
    assert!(matches!(
        chain.push(earlier),
        Err(ChainError::BrokenLink { .. })
    ));
}

#[test]
fn chain_broken_link_index_is_correct() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("a", 100, Outcome::Complete))
        .unwrap();
    let earlier = hashed_receipt_at("b", 0, Outcome::Complete);
    match chain.push(earlier) {
        Err(ChainError::BrokenLink { index }) => assert_eq!(index, 1),
        other => panic!("expected BrokenLink, got {other:?}"),
    }
}

#[test]
fn chain_same_timestamp_allowed() {
    let mut chain = ReceiptChain::new();
    chain
        .push(hashed_receipt_at("a", 0, Outcome::Complete))
        .unwrap();
    // Same offset = same started_at; not strictly before, so it should pass.
    chain
        .push(hashed_receipt_at("b", 0, Outcome::Complete))
        .unwrap();
    assert_eq!(chain.len(), 2);
}

// ===========================================================================
// ReceiptChain — chain accumulation
// ===========================================================================

#[test]
fn chain_accumulate_50_receipts() {
    let mut chain = ReceiptChain::new();
    for i in 0..50 {
        chain
            .push(hashed_receipt_at("bulk", i, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 50);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_accumulate_mixed_outcomes() {
    let mut chain = ReceiptChain::new();
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    for i in 0..9 {
        chain
            .push(hashed_receipt_at(
                "m",
                i * 5,
                outcomes[i as usize % 3].clone(),
            ))
            .unwrap();
    }
    assert_eq!(chain.len(), 9);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_accumulate_mixed_backends() {
    let mut chain = ReceiptChain::new();
    let backends = ["openai", "claude", "gemini", "local"];
    for (i, b) in backends.iter().enumerate() {
        chain
            .push(hashed_receipt_at(b, (i as i64) * 10, Outcome::Complete))
            .unwrap();
    }
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids, backends.to_vec());
}

// ===========================================================================
// ReceiptChain — latest receipt retrieval
// ===========================================================================

#[test]
fn chain_latest_empty_is_none() {
    assert!(ReceiptChain::new().latest().is_none());
}

#[test]
fn chain_latest_single() {
    let mut chain = ReceiptChain::new();
    let r = hashed_receipt_at("only", 0, Outcome::Complete);
    let id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, id);
}

#[test]
fn chain_latest_tracks_last() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(hashed_receipt_at("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    let last = chain.latest().unwrap();
    assert_eq!(last.meta.started_at, time_offset(40));
}

// ===========================================================================
// ReceiptChain — ChainError display
// ===========================================================================

#[test]
fn chain_error_empty_chain_display() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
}

#[test]
fn chain_error_hash_mismatch_display() {
    let e = ChainError::HashMismatch { index: 7 };
    assert_eq!(e.to_string(), "hash mismatch at chain index 7");
}

#[test]
fn chain_error_broken_link_display() {
    let e = ChainError::BrokenLink { index: 2 };
    assert_eq!(e.to_string(), "broken link at chain index 2");
}

#[test]
fn chain_error_duplicate_id_display() {
    let id = Uuid::nil();
    let e = ChainError::DuplicateId { id };
    assert!(e.to_string().contains("duplicate"));
    assert!(e.to_string().contains(&id.to_string()));
}

#[test]
fn chain_error_is_std_error() {
    let e: Box<dyn std::error::Error> = Box::new(ChainError::EmptyChain);
    assert!(!e.to_string().is_empty());
}

#[test]
fn chain_error_eq() {
    assert_eq!(ChainError::EmptyChain, ChainError::EmptyChain);
    assert_ne!(
        ChainError::HashMismatch { index: 0 },
        ChainError::HashMismatch { index: 1 }
    );
}

#[test]
fn chain_error_clone() {
    let e = ChainError::BrokenLink { index: 3 };
    let cloned = e.clone();
    assert_eq!(e, cloned);
}

// ===========================================================================
// FileStore — construction and basic save/load
// ===========================================================================

#[test]
fn file_store_new() {
    let dir = tempfile::tempdir().unwrap();
    let _store = ReceiptStore::new(dir.path());
}

#[test]
fn file_store_save_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let path = store.save(&r).unwrap();
    assert!(path.exists());
}

#[test]
fn file_store_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.meta.run_id, id);
    assert_eq!(loaded.outcome, r.outcome);
    assert_eq!(loaded.backend.id, r.backend.id);
}

#[test]
fn file_store_save_filename_is_uuid_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    let path = store.save(&r).unwrap();
    let expected = dir.path().join(format!("{id}.json"));
    assert_eq!(path, expected);
}

#[test]
fn file_store_save_preserves_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let original_hash = r.receipt_sha256.clone();
    store.save(&r).unwrap();
    let loaded = store.load(r.meta.run_id).unwrap();
    assert_eq!(loaded.receipt_sha256, original_hash);
}

// ===========================================================================
// FileStore — store multiple receipts
// ===========================================================================

#[test]
fn file_store_save_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..5 {
        let r = hashed_receipt_at("m", i * 10, Outcome::Complete);
        store.save(&r).unwrap();
    }
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 5);
}

#[test]
fn file_store_load_each_after_multi_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let mut saved_ids = Vec::new();
    for i in 0..3 {
        let r = hashed_receipt_at("m", i * 10, Outcome::Complete);
        saved_ids.push(r.meta.run_id);
        store.save(&r).unwrap();
    }
    for id in &saved_ids {
        let loaded = store.load(*id).unwrap();
        assert_eq!(loaded.meta.run_id, *id);
    }
}

// ===========================================================================
// FileStore — list receipts
// ===========================================================================

#[test]
fn file_store_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let ids = store.list().unwrap();
    assert!(ids.is_empty());
}

#[test]
fn file_store_list_nonexistent_dir() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path().join("nonexistent"));
    let ids = store.list().unwrap();
    assert!(ids.is_empty());
}

#[test]
fn file_store_list_returns_all_saved_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let mut expected = Vec::new();
    for i in 0..4 {
        let r = hashed_receipt_at("m", i * 10, Outcome::Complete);
        expected.push(r.meta.run_id);
        store.save(&r).unwrap();
    }
    let mut ids = store.list().unwrap();
    expected.sort();
    ids.sort();
    assert_eq!(ids, expected);
}

#[test]
fn file_store_list_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..5 {
        store
            .save(&hashed_receipt_at("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    let ids = store.list().unwrap();
    let sorted = {
        let mut s = ids.clone();
        s.sort();
        s
    };
    assert_eq!(ids, sorted);
}

#[test]
fn file_store_list_ignores_non_json_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    store
        .save(&hashed_receipt_at("m", 0, Outcome::Complete))
        .unwrap();
    // Write a non-JSON file that should be ignored.
    std::fs::write(dir.path().join("readme.txt"), "not a receipt").unwrap();
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
}

#[test]
fn file_store_list_ignores_non_uuid_json_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    store
        .save(&hashed_receipt_at("m", 0, Outcome::Complete))
        .unwrap();
    std::fs::write(dir.path().join("not-a-uuid.json"), "{}").unwrap();
    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
}

// ===========================================================================
// FileStore — verify single receipt
// ===========================================================================

#[test]
fn file_store_verify_valid_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    assert!(store.verify(id).unwrap());
}

#[test]
fn file_store_verify_unhashed_receipt_is_false() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = plain_receipt_at("m", 0);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    // Without a stored hash, `verify` compares None with Some(computed) → false.
    assert!(!store.verify(id).unwrap());
}

#[test]
fn file_store_verify_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let result = store.verify(Uuid::new_v4());
    assert!(result.is_err());
}

// ===========================================================================
// FileStore — verify_chain
// ===========================================================================

#[test]
fn file_store_verify_chain_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let v = store.verify_chain().unwrap();
    assert_eq!(v.valid_count, 0);
    assert!(v.invalid_hashes.is_empty());
    assert!(v.gaps.is_empty());
    assert!(v.is_valid);
}

#[test]
fn file_store_verify_chain_single_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    store
        .save(&hashed_receipt_at("m", 0, Outcome::Complete))
        .unwrap();
    let v = store.verify_chain().unwrap();
    assert_eq!(v.valid_count, 1);
    assert!(v.is_valid);
}

#[test]
fn file_store_verify_chain_multiple_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..3 {
        store
            .save(&hashed_receipt_at("m", i * 60, Outcome::Complete))
            .unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert_eq!(v.valid_count, 3);
    assert!(v.is_valid);
    assert_eq!(v.gaps.len(), 2);
}

#[test]
fn file_store_verify_chain_detects_invalid_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    // Save a valid receipt.
    store
        .save(&hashed_receipt_at("good", 0, Outcome::Complete))
        .unwrap();
    // Save a receipt with an incorrect hash.
    let mut bad = hashed_receipt_at("bad", 60, Outcome::Complete);
    bad.outcome = Outcome::Failed; // tamper after hashing
    store.save(&bad).unwrap();
    let v = store.verify_chain().unwrap();
    assert!(!v.is_valid);
    assert_eq!(v.invalid_hashes.len(), 1);
}

// ===========================================================================
// FileStore — overwrite / re-save
// ===========================================================================

#[test]
fn file_store_overwrite_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();

    // Rebuild with same run_id but different outcome.
    let start = time_offset(0);
    let finish = start + Duration::milliseconds(100);
    let r2 = ReceiptBuilder::new("m")
        .run_id(id)
        .outcome(Outcome::Failed)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap();
    store.save(&r2).unwrap();

    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.outcome, Outcome::Failed);
}

// ===========================================================================
// FileStore — error handling (not found, corruption)
// ===========================================================================

#[test]
fn file_store_load_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let result = store.load(Uuid::new_v4());
    assert!(result.is_err());
}

#[test]
fn file_store_load_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let id = Uuid::new_v4();
    let path = dir.path().join(format!("{id}.json"));
    std::fs::write(&path, "this is not valid JSON").unwrap();
    let result = store.load(id);
    assert!(result.is_err());
}

#[test]
fn file_store_load_wrong_structure_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let id = Uuid::new_v4();
    let path = dir.path().join(format!("{id}.json"));
    std::fs::write(&path, r#"{"not": "a receipt"}"#).unwrap();
    let result = store.load(id);
    assert!(result.is_err());
}

// ===========================================================================
// Receipt hash verification on retrieval
// ===========================================================================

#[test]
fn file_store_loaded_receipt_hash_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert!(verify_hash(&loaded));
}

#[test]
fn file_store_loaded_receipt_compute_hash_matches() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    let recomputed = compute_hash(&loaded).unwrap();
    assert_eq!(loaded.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}

#[test]
fn file_store_tampered_file_fails_verify() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    let path = store.save(&r).unwrap();

    // Tamper with the file on disk.
    let mut json = std::fs::read_to_string(&path).unwrap();
    json = json.replace("\"complete\"", "\"failed\"");
    std::fs::write(&path, json).unwrap();

    let loaded = store.load(id).unwrap();
    assert!(!verify_hash(&loaded));
}

// ===========================================================================
// Store serde roundtrip
// ===========================================================================

#[test]
fn serde_roundtrip_receipt_json() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.meta.run_id, r.meta.run_id);
    assert_eq!(deser.receipt_sha256, r.receipt_sha256);
    assert_eq!(deser.outcome, r.outcome);
}

#[test]
fn serde_roundtrip_preserves_hash_validity() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deser));
}

#[test]
fn serde_roundtrip_pretty_json() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let pretty = serde_json::to_string_pretty(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&pretty).unwrap();
    assert_eq!(deser.meta.run_id, r.meta.run_id);
    assert!(verify_hash(&deser));
}

#[test]
fn serde_roundtrip_canonical_form() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    let c2 = canonicalize(&deser).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn serde_roundtrip_full_receipt_with_trace() {
    let r = ReceiptBuilder::new("full")
        .outcome(Outcome::Complete)
        .started_at(base_time())
        .finished_at(time_offset(5))
        .backend_version("1.0")
        .adapter_version("0.1")
        .usage_raw(serde_json::json!({"tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git a/b".into()),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        })
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deser));
    assert_eq!(deser.trace.len(), 1);
    assert_eq!(deser.artifacts.len(), 1);
    assert_eq!(deser.backend.backend_version.as_deref(), Some("1.0"));
}

// ===========================================================================
// FileStore — capacity / many receipts
// ===========================================================================

#[test]
fn file_store_100_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..100 {
        store
            .save(&hashed_receipt_at("m", i, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(store.list().unwrap().len(), 100);
}

#[test]
fn file_store_verify_chain_100_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..100 {
        store
            .save(&hashed_receipt_at("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert_eq!(v.valid_count, 100);
    assert!(v.is_valid);
}

// ===========================================================================
// Concurrent store access (sequential multi-reader pattern)
// ===========================================================================

#[test]
fn file_store_concurrent_read_after_write() {
    let dir = tempfile::tempdir().unwrap();
    let store1 = ReceiptStore::new(dir.path());
    let store2 = ReceiptStore::new(dir.path());

    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store1.save(&r).unwrap();

    // Second store instance can read what first wrote.
    let loaded = store2.load(id).unwrap();
    assert_eq!(loaded.meta.run_id, id);
}

#[test]
fn file_store_two_writers_different_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store1 = ReceiptStore::new(dir.path());
    let store2 = ReceiptStore::new(dir.path());

    let r1 = hashed_receipt_at("a", 0, Outcome::Complete);
    let r2 = hashed_receipt_at("b", 10, Outcome::Complete);

    store1.save(&r1).unwrap();
    store2.save(&r2).unwrap();

    let ids = store1.list().unwrap();
    assert_eq!(ids.len(), 2);
}

#[test]
fn file_store_list_after_external_add() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;

    // Write receipt externally (simulating another process).
    let json = serde_json::to_string_pretty(&r).unwrap();
    std::fs::write(dir.path().join(format!("{id}.json")), json).unwrap();

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], id);
}

// ===========================================================================
// Hash functions standalone
// ===========================================================================

#[test]
fn compute_hash_deterministic() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn compute_hash_differs_for_different_outcomes() {
    let r1 = plain_receipt_at("m", 0);
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn canonicalize_nullifies_hash_field() {
    let mut r = hashed_receipt_at("m", 0, Outcome::Complete);
    assert!(r.receipt_sha256.is_some());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));

    // Removing the hash gives same canonical form.
    r.receipt_sha256 = None;
    let json2 = canonicalize(&r).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn canonicalize_is_compact() {
    let r = plain_receipt_at("m", 0);
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
}

#[test]
fn verify_hash_true_for_none_hash() {
    let r = plain_receipt_at("m", 0);
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_true_for_correct_hash() {
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_false_for_tampered() {
    let mut r = hashed_receipt_at("m", 0, Outcome::Complete);
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_false_for_garbage() {
    let mut r = plain_receipt_at("m", 0);
    r.receipt_sha256 = Some("cafebabe".into());
    assert!(!verify_hash(&r));
}

#[test]
fn compute_hash_matches_core_receipt_hash() {
    let r = plain_receipt_at("m", 0);
    let h1 = compute_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

// ===========================================================================
// Diff integration with store roundtrip
// ===========================================================================

#[test]
fn diff_after_store_roundtrip_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    let d = diff_receipts(&r, &loaded);
    assert!(d.is_empty());
}

// ===========================================================================
// FileStore — subdirectory creation
// ===========================================================================

#[test]
fn file_store_creates_directory_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("deep").join("receipts");
    let store = ReceiptStore::new(&nested);
    let r = hashed_receipt_at("m", 0, Outcome::Complete);
    store.save(&r).unwrap();
    assert!(nested.exists());
    assert_eq!(store.list().unwrap().len(), 1);
}

// ===========================================================================
// ReceiptChain + FileStore integration
// ===========================================================================

#[test]
fn chain_from_file_store_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    // Save receipts to file store.
    let mut saved = Vec::new();
    for i in 0..3 {
        let r = hashed_receipt_at("m", i * 60, Outcome::Complete);
        saved.push(r.meta.run_id);
        store.save(&r).unwrap();
    }

    // Load all and push into chain ordered by started_at.
    let mut chain = ReceiptChain::new();
    let mut receipts: Vec<Receipt> = saved.iter().map(|id| store.load(*id).unwrap()).collect();
    receipts.sort_by_key(|r| r.meta.started_at);
    for r in receipts {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn file_store_verify_chain_gaps_are_chronological() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    // Receipts at 0s, 60s, 120s (each 100ms long).
    for i in 0..3 {
        store
            .save(&hashed_receipt_at("m", i * 60, Outcome::Complete))
            .unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert_eq!(v.gaps.len(), 2);
    // Each gap's second timestamp should be after the first.
    for (gap_end, gap_start) in &v.gaps {
        assert!(gap_start >= gap_end);
    }
}

// ===========================================================================
// Edge: unicode, empty backend, large trace
// ===========================================================================

#[test]
fn file_store_unicode_backend() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = ReceiptBuilder::new("バックエンド🚀")
        .outcome(Outcome::Complete)
        .started_at(base_time())
        .finished_at(time_offset(1))
        .with_hash()
        .unwrap();
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.backend.id, "バックエンド🚀");
    assert!(verify_hash(&loaded));
}

#[test]
fn file_store_empty_backend_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = ReceiptBuilder::new("")
        .outcome(Outcome::Complete)
        .started_at(base_time())
        .finished_at(time_offset(1))
        .with_hash()
        .unwrap();
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.backend.id, "");
    assert!(verify_hash(&loaded));
}

#[test]
fn file_store_large_trace_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let mut builder = ReceiptBuilder::new("m")
        .started_at(base_time())
        .finished_at(time_offset(10));
    for i in 0..200 {
        builder = builder.add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.trace.len(), 200);
    assert!(verify_hash(&loaded));
}

// ===========================================================================
// ReceiptBuilder (via abp_receipt) edge cases
// ===========================================================================

#[test]
fn builder_with_hash_produces_valid_receipt() {
    let r = ReceiptBuilder::new("test").with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn builder_build_without_hash() {
    let r = ReceiptBuilder::new("test").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn builder_run_id_deterministic() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("test").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

// ===========================================================================
// Receipt construction with all fields
// ===========================================================================

#[test]
fn receipt_construction_all_outcome_variants() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("test")
            .outcome(outcome.clone())
            .build();
        assert_eq!(r.outcome, outcome);
    }
}

#[test]
fn receipt_construction_with_capabilities() {
    use abp_core::{Capability, SupportLevel};
    let mut caps = abp_core::CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let r = ReceiptBuilder::new("caps-test").capabilities(caps.clone()).build();
    assert_eq!(r.capabilities.len(), 2);
    assert!(r.capabilities.contains_key(&Capability::ToolRead));
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn receipt_construction_execution_mode_passthrough() {
    use abp_core::ExecutionMode;
    let r = ReceiptBuilder::new("test")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_construction_execution_mode_mapped_default() {
    use abp_core::ExecutionMode;
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn receipt_construction_with_work_order_id() {
    let wo_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("test").work_order_id(wo_id).build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn receipt_construction_complex_usage_raw() {
    let raw = serde_json::json!({
        "prompt_tokens": 1500,
        "completion_tokens": 800,
        "model": "gpt-4",
        "nested": { "cache_hit": true, "details": [1, 2, 3] }
    });
    let r = ReceiptBuilder::new("test").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn receipt_construction_all_usage_normalized_fields() {
    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(42),
        estimated_cost_usd: Some(0.05),
    };
    let r = ReceiptBuilder::new("test").usage(usage).build();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(500));
    assert_eq!(r.usage.cache_read_tokens, Some(200));
    assert_eq!(r.usage.cache_write_tokens, Some(100));
    assert_eq!(r.usage.request_units, Some(42));
    assert!((r.usage.estimated_cost_usd.unwrap() - 0.05).abs() < f64::EPSILON);
}

#[test]
fn receipt_construction_verification_report_all_fields() {
    let vr = VerificationReport {
        git_diff: Some("diff --git a/foo.rs b/foo.rs".into()),
        git_status: Some("M foo.rs".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("test").verification(vr).build();
    assert!(r.verification.harness_ok);
    assert_eq!(
        r.verification.git_diff.as_deref(),
        Some("diff --git a/foo.rs b/foo.rs")
    );
    assert_eq!(r.verification.git_status.as_deref(), Some("M foo.rs"));
}

#[test]
fn receipt_construction_multiple_artifacts() {
    let r = ReceiptBuilder::new("test")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "build.log".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "diff".into(),
            path: "changes.diff".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 3);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert_eq!(r.artifacts[2].path, "changes.diff");
}

#[test]
fn receipt_construction_contract_version() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.meta.contract_version, abp_core::CONTRACT_VERSION);
}

// ===========================================================================
// receipt_hash() determinism
// ===========================================================================

#[test]
fn receipt_hash_deterministic_repeated() {
    let r = plain_receipt_at("determinism-test", 0);
    let hashes: Vec<_> = (0..10).map(|_| receipt_hash(&r).unwrap()).collect();
    for h in &hashes {
        assert_eq!(h, &hashes[0]);
    }
}

#[test]
fn receipt_hash_deterministic_for_clone() {
    let r = plain_receipt_at("clone-test", 0);
    let cloned = r.clone();
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&cloned).unwrap());
}

#[test]
fn receipt_hash_differs_for_different_backend() {
    let r1 = plain_receipt_at("backend-a", 0);
    let r2 = plain_receipt_at("backend-b", 0);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_hash_differs_for_different_timing() {
    let r1 = plain_receipt_at("m", 0);
    let r2 = plain_receipt_at("m", 100);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// with_hash() produces valid SHA-256
// ===========================================================================

#[test]
fn with_hash_hex_characters_only() {
    let r = ReceiptBuilder::new("hex-test").build().with_hash().unwrap();
    let hash = r.receipt_sha256.unwrap();
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn with_hash_is_lowercase_hex() {
    let r = ReceiptBuilder::new("case-test").build().with_hash().unwrap();
    let hash = r.receipt_sha256.unwrap();
    assert_eq!(hash, hash.to_lowercase());
}

#[test]
fn with_hash_exactly_64_chars() {
    let r = ReceiptBuilder::new("len-test").build().with_hash().unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn with_hash_on_receipt_struct_directly() {
    let r = plain_receipt_at("direct", 0).with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

// ===========================================================================
// receipt_hash() nulls receipt_sha256 before hashing (self-referential)
// ===========================================================================

#[test]
fn receipt_hash_ignores_existing_hash_value() {
    let mut r = plain_receipt_at("m", 0);
    let h1 = receipt_hash(&r).unwrap();
    r.receipt_sha256 = Some("some_arbitrary_value".into());
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_with_correct_hash_set_equals_without() {
    let r = plain_receipt_at("m", 0);
    let h_none = receipt_hash(&r).unwrap();
    let mut r_with = r.clone();
    r_with.receipt_sha256 = Some(h_none.clone());
    let h_with = receipt_hash(&r_with).unwrap();
    assert_eq!(h_none, h_with);
}

#[test]
fn canonicalize_always_nulls_regardless_of_input() {
    let mut r = hashed_receipt_at("m", 0, Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    r.receipt_sha256 = Some("different_value".into());
    let c2 = canonicalize(&r).unwrap();
    r.receipt_sha256 = None;
    let c3 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
    assert_eq!(c2, c3);
}

// ===========================================================================
// Receipt serde roundtrip preserves all fields
// ===========================================================================

#[test]
fn serde_roundtrip_preserves_usage_normalized() {
    let r = ReceiptBuilder::new("usage-rt")
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(25),
            request_units: Some(10),
            estimated_cost_usd: Some(0.123),
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.usage.input_tokens, Some(100));
    assert_eq!(deser.usage.output_tokens, Some(200));
    assert_eq!(deser.usage.cache_read_tokens, Some(50));
    assert_eq!(deser.usage.cache_write_tokens, Some(25));
    assert_eq!(deser.usage.request_units, Some(10));
    assert!((deser.usage.estimated_cost_usd.unwrap() - 0.123).abs() < f64::EPSILON);
}

#[test]
fn serde_roundtrip_preserves_execution_mode() {
    use abp_core::ExecutionMode;
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let r = ReceiptBuilder::new("mode-rt").mode(mode).build();
        let json = serde_json::to_string(&r).unwrap();
        let deser: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.mode, mode);
    }
}

#[test]
fn serde_roundtrip_preserves_artifacts() {
    let r = ReceiptBuilder::new("art-rt")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "out.log".into(),
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.artifacts.len(), 2);
    assert_eq!(deser.artifacts[0].kind, "patch");
    assert_eq!(deser.artifacts[1].path, "out.log");
}

#[test]
fn serde_roundtrip_preserves_verification_report() {
    let r = ReceiptBuilder::new("vr-rt")
        .verification(VerificationReport {
            git_diff: Some("diff data".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.verification.git_diff.as_deref(), Some("diff data"));
    assert_eq!(
        deser.verification.git_status.as_deref(),
        Some("M file.rs")
    );
    assert!(deser.verification.harness_ok);
}

#[test]
fn serde_roundtrip_preserves_backend_identity() {
    let r = ReceiptBuilder::new("backend-rt")
        .backend_version("2.0.0")
        .adapter_version("1.5.0")
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.backend.id, "backend-rt");
    assert_eq!(deser.backend.backend_version.as_deref(), Some("2.0.0"));
    assert_eq!(deser.backend.adapter_version.as_deref(), Some("1.5.0"));
}

// ===========================================================================
// BTreeMap ordering for deterministic JSON
// ===========================================================================

#[test]
fn canonical_json_keys_sorted() {
    let r = plain_receipt_at("btree-test", 0);
    let json = canonicalize(&r).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = v {
        let keys: Vec<_> = map.keys().cloned().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    } else {
        panic!("expected object");
    }
}

#[test]
fn btreemap_vendor_fields_deterministic_order() {
    use std::collections::BTreeMap;
    let mut vendor = BTreeMap::new();
    vendor.insert("zebra".to_string(), serde_json::json!(1));
    vendor.insert("alpha".to_string(), serde_json::json!(2));
    vendor.insert("middle".to_string(), serde_json::json!(3));
    let r = ReceiptBuilder::new("vendor-order")
        .usage_raw(serde_json::json!(vendor))
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let middle_pos = json.find("\"middle\"").unwrap();
    let zebra_pos = json.find("\"zebra\"").unwrap();
    assert!(alpha_pos < middle_pos);
    assert!(middle_pos < zebra_pos);
}

#[test]
fn capabilities_serialized_deterministic_order() {
    use abp_core::{Capability, SupportLevel};
    let mut caps = abp_core::CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    let r = ReceiptBuilder::new("caps-order").capabilities(caps).build();
    let json1 = serde_json::to_string(&r).unwrap();
    let json2 = serde_json::to_string(&r).unwrap();
    assert_eq!(json1, json2);
}

// ===========================================================================
// Receipt with events, metrics, timing data
// ===========================================================================

#[test]
fn receipt_with_tool_call_event() {
    let r = ReceiptBuilder::new("events")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/src/main.rs"}),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert_eq!(r.trace.len(), 1);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_tool_result_event() {
    let r = ReceiptBuilder::new("events")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_file_changed_event() {
    let r = ReceiptBuilder::new("events")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added error handling".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_command_executed_event() {
    let r = ReceiptBuilder::new("events")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok".into()),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_warning_and_error_events() {
    let r = ReceiptBuilder::new("events")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::Warning {
                message: "deprecated API".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::Error {
                message: "compilation failed".into(),
                error_code: None,
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    assert_eq!(r.trace.len(), 2);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_all_event_types_roundtrip() {
    let events = vec![
        AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello world".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let mut builder = ReceiptBuilder::new("all-events")
        .started_at(base_time())
        .finished_at(time_offset(10));
    for e in events {
        builder = builder.add_trace_event(e);
    }
    let r = builder.with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.trace.len(), 4);
    assert!(verify_hash(&deser));
}

#[test]
fn receipt_duration_ms_computed_correctly() {
    let start = base_time();
    let finish = start + Duration::milliseconds(1234);
    let r = ReceiptBuilder::new("timing")
        .started_at(start)
        .finished_at(finish)
        .build();
    assert_eq!(r.meta.duration_ms, 1234);
}

#[test]
fn receipt_zero_duration() {
    let t = base_time();
    let r = ReceiptBuilder::new("zero-dur")
        .started_at(t)
        .finished_at(t)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

// ===========================================================================
// Edge cases: empty events, null optional fields, very large receipts
// ===========================================================================

#[test]
fn receipt_empty_trace_hashes_ok() {
    let r = ReceiptBuilder::new("empty-trace").build();
    assert!(r.trace.is_empty());
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_empty_artifacts_hashes_ok() {
    let r = ReceiptBuilder::new("empty-art").build();
    assert!(r.artifacts.is_empty());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_all_usage_fields_none() {
    let r = ReceiptBuilder::new("null-usage")
        .usage(UsageNormalized::default())
        .build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
    assert!(r.usage.request_units.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_none_backend_versions() {
    let r = ReceiptBuilder::new("no-versions").build();
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_none_verification_fields() {
    let r = ReceiptBuilder::new("no-verify").build();
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
    assert!(!r.verification.harness_ok);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_very_large_usage_raw() {
    let mut big = serde_json::Map::new();
    for i in 0..500 {
        big.insert(format!("key_{i:04}"), serde_json::json!(i));
    }
    let r = ReceiptBuilder::new("large-raw")
        .usage_raw(serde_json::Value::Object(big))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn receipt_many_artifacts() {
    let mut builder = ReceiptBuilder::new("many-art");
    for i in 0..100 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "file".into(),
            path: format!("artifact_{i}.txt"),
        });
    }
    let r = builder.with_hash().unwrap();
    assert_eq!(r.artifacts.len(), 100);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_event_with_ext_field() {
    use std::collections::BTreeMap;
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor_field": "data"}),
    );
    let r = ReceiptBuilder::new("ext-test")
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        })
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert!(deser.trace[0].ext.is_some());
    assert!(verify_hash(&deser));
}

#[test]
fn diff_detects_outcome_change() {
    let r1 = ReceiptBuilder::new("diff-test")
        .outcome(Outcome::Complete)
        .started_at(base_time())
        .finished_at(time_offset(1))
        .build();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    let d = diff_receipts(&r1, &r2);
    assert!(!d.is_empty());
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn diff_detects_backend_id_change() {
    let r1 = ReceiptBuilder::new("a")
        .started_at(base_time())
        .finished_at(time_offset(1))
        .build();
    let mut r2 = r1.clone();
    r2.backend.id = "b".into();
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_identical_receipts_empty() {
    let r = plain_receipt_at("identical", 0);
    let d = diff_receipts(&r, &r);
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn file_store_save_and_verify_all_event_types() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = ReceiptBuilder::new("full-events")
        .started_at(base_time())
        .finished_at(time_offset(5))
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .with_hash()
        .unwrap();
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.trace.len(), 3);
    assert!(verify_hash(&loaded));
}
