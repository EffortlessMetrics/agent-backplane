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
